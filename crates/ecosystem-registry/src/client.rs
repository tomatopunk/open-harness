//! Registry client implementation

use crate::cache::RegistryCache;
use crate::search::{process_search_results, SearchQuery};
use crate::types::{
    ComponentMetadata, RegistryConfig, RegistryError, RegistryResult, SearchResult,
};
use reqwest::Client as HttpClient;
use std::path::PathBuf;
use tracing::debug;

/// Registry client
pub struct RegistryClient {
    /// Registry configuration
    config: RegistryConfig,
    /// HTTP client
    http_client: HttpClient,
    /// Cache
    cache: Option<RegistryCache>,
}

impl RegistryClient {
    /// Create new registry client
    pub fn new(config: RegistryConfig) -> Self {
        let http_client = HttpClient::new();
        Self { config, http_client, cache: None }
    }

    /// Enable caching
    pub fn with_cache(mut self, cache_dir: PathBuf, ttl_seconds: u64) -> Self {
        self.cache = Some(RegistryCache::new(cache_dir, ttl_seconds));
        self
    }

    /// Initialize client (creates cache directory if needed)
    pub async fn initialize(&self) -> RegistryResult<()> {
        if let Some(cache) = &self.cache {
            cache.initialize().await?;
        }
        Ok(())
    }

    /// Search for components
    pub async fn search(&self, query: &SearchQuery) -> RegistryResult<SearchResult> {
        // Check in-memory cache first
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get_search(&format!("{:?}", query)) {
                debug!("Search cache hit for query: {:?}", query);
                return Ok(SearchResult {
                    total: cached.len(),
                    items: cached,
                    page: query.page,
                    per_page: query.per_page,
                });
            }
        }

        // Build request URL
        let url = format!("{}/api/v1/search", self.config.url.trim_end_matches('/'));
        let mut query_params = Vec::new();

        if let Some(q) = &query.query {
            query_params.push(("q".to_string(), q.clone()));
        }
        if let Some(ty) = &query.component_type {
            let ty_str = match ty {
                crate::types::ComponentType::McpServer => "mcp-server",
                crate::types::ComponentType::Skill => "skill",
                crate::types::ComponentType::Plugin => "plugin",
            };
            query_params.push(("type".to_string(), ty_str.to_string()));
        }
        query_params.push(("page".to_string(), query.page.to_string()));
        query_params.push(("per_page".to_string(), query.per_page.to_string()));

        // Send request
        let mut request = self.http_client.get(&url);
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }

        request = request.query(&query_params);

        debug!("Sending search request to: {}", url);
        let response = request
            .send()
            .await
            .map_err(|e| RegistryError::HttpRequest(format!("Failed to search: {}", e)))?;

        if !response.status().is_success() {
            return Err(RegistryError::ApiError(format!(
                "Search failed with status: {}",
                response.status()
            )));
        }

        let results: SearchResult = response.json().await.map_err(|e| {
            RegistryError::JsonDeserialize(format!("Failed to parse search response: {}", e))
        })?;

        // Cache results
        if let Some(cache) = &self.cache {
            cache.store_search(format!("{:?}", query), results.items.clone());
        }

        Ok(results)
    }

    /// Get component metadata by ID
    pub async fn get_component(&self, id: &str) -> RegistryResult<ComponentMetadata> {
        // Check cache first
        if let Some(cache) = &self.cache {
            if let Ok(Some(cached)) = cache.get_component(id).await {
                debug!("Cache hit for component: {}", id);
                return Ok(cached);
            }
        }

        // Fetch from registry
        let url = format!("{}/api/v1/components/{}", self.config.url.trim_end_matches('/'), id);

        let mut request = self.http_client.get(&url);
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| RegistryError::HttpRequest(format!("Failed to get component: {}", e)))?;

        if response.status().is_client_error() {
            return Err(RegistryError::ComponentNotFound(id.to_string()));
        }

        if !response.status().is_success() {
            return Err(RegistryError::ApiError(format!(
                "Get component failed with status: {}",
                response.status()
            )));
        }

        let metadata: ComponentMetadata = response.json().await.map_err(|e| {
            RegistryError::JsonDeserialize(format!("Failed to parse component: {}", e))
        })?;

        // Cache the result
        if let Some(cache) = &self.cache {
            let _ = cache.store_component(id, &metadata).await;
        }

        Ok(metadata)
    }

    /// Get component by ID and version
    pub async fn get_component_version(
        &self,
        id: &str,
        version: &str,
    ) -> RegistryResult<ComponentMetadata> {
        // First get current metadata, which contains all versions
        let metadata = self.get_component(id).await?;

        if !metadata.versions.contains(&version.to_string()) {
            return Err(RegistryError::VersionNotFound(format!("{id}@{version}")));
        }

        // Fetch specific version
        let url = format!(
            "{}/api/v1/components/{}/versions/{}",
            self.config.url.trim_end_matches('/'),
            id,
            version
        );

        let mut request = self.http_client.get(&url);
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            RegistryError::HttpRequest(format!("Failed to get component version: {}", e))
        })?;

        if response.status().is_client_error() {
            return Err(RegistryError::VersionNotFound(format!("{id}@{version}")));
        }

        if !response.status().is_success() {
            return Err(RegistryError::ApiError(format!(
                "Get version failed with status: {}",
                response.status()
            )));
        }

        let version_metadata: ComponentMetadata = response.json().await.map_err(|e| {
            RegistryError::JsonDeserialize(format!("Failed to parse version: {}", e))
        })?;

        Ok(version_metadata)
    }

    /// Get registry name
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get registry URL
    pub fn url(&self) -> &str {
        &self.config.url
    }
}

/// Multi-registry client that aggregates results from multiple registries
pub struct MultiRegistryClient {
    registries: Vec<RegistryClient>,
    cache_root: PathBuf,
}

impl MultiRegistryClient {
    /// Create new multi-registry client
    pub fn new(configs: Vec<RegistryConfig>, cache_root: PathBuf) -> Self {
        let mut registries = Vec::new();

        for (i, config) in configs.into_iter().enumerate() {
            let cache_dir = cache_root.join(format!("registry_{}", i));
            let client = RegistryClient::new(config).with_cache(cache_dir, 3600); // 1 hour TTL
            registries.push(client);
        }

        Self { registries, cache_root }
    }

    /// Initialize all registries
    pub async fn initialize(&self) -> RegistryResult<()> {
        if !self.cache_root.exists() {
            tokio::fs::create_dir_all(&self.cache_root).await.map_err(|e| {
                RegistryError::CacheError(format!("Failed to create cache root: {}", e))
            })?;
        }

        for client in &self.registries {
            client.initialize().await?;
        }

        Ok(())
    }

    /// Search across all registries
    pub async fn search(&self, query: &SearchQuery) -> RegistryResult<SearchResult> {
        let mut all_items = Vec::new();

        for client in &self.registries {
            match client.search(query).await {
                Ok(result) => {
                    all_items.extend(result.items);
                }
                Err(e) => {
                    tracing::warn!("Search failed on registry '{}': {}", client.name(), e);
                }
            }
        }

        // Process results (sort, filter, paginate)
        let processed = process_search_results(all_items, query);

        Ok(processed)
    }

    /// Find a component across all registries
    pub async fn find_component(&self, id: &str) -> RegistryResult<(usize, ComponentMetadata)> {
        for (i, client) in self.registries.iter().enumerate() {
            match client.get_component(id).await {
                Ok(metadata) => {
                    return Ok((i, metadata));
                }
                Err(RegistryError::ComponentNotFound(_)) => {
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to get component from registry '{}': {}",
                        client.name(),
                        e
                    );
                    continue;
                }
            }
        }

        Err(RegistryError::ComponentNotFound(id.to_string()))
    }

    /// Get all registries
    pub fn registries(&self) -> &[RegistryClient] {
        &self.registries
    }
}
