//! Metadata cache for registry client

use crate::types::{ComponentMetadata, RegistryError, RegistryResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

/// Cache entry with expiration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry<T> {
    /// Cached data
    data: T,
    /// Expiration timestamp (unix seconds)
    expires_at: u64,
}

/// Metadata cache
pub struct RegistryCache {
    /// Cache directory
    cache_dir: PathBuf,
    /// TTL in seconds
    ttl: u64,
    /// In-memory cache for search results
    search_cache: parking_lot::RwLock<HashMap<String, CacheEntry<Vec<ComponentMetadata>>>>,
}

impl RegistryCache {
    /// Create new cache instance
    pub fn new(cache_dir: PathBuf, ttl_seconds: u64) -> Self {
        Self { cache_dir, ttl: ttl_seconds, search_cache: parking_lot::RwLock::new(HashMap::new()) }
    }

    /// Initialize cache directory
    pub async fn initialize(&self) -> RegistryResult<()> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).await.map_err(|e| {
                RegistryError::CacheError(format!("Failed to create cache dir: {}", e))
            })?;
        }
        Ok(())
    }

    /// Get cached component metadata
    pub async fn get_component(&self, id: &str) -> RegistryResult<Option<ComponentMetadata>> {
        let cache_path = self.cache_dir.join(format!("component_{}.json", id));

        if !cache_path.exists() {
            return Ok(None);
        }

        let content = match fs::read_to_string(&cache_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read cache: {}", e);
                return Ok(None);
            }
        };

        let entry: CacheEntry<ComponentMetadata> = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to deserialize cache: {}", e);
                return Ok(None);
            }
        };

        if self.is_expired(entry.expires_at) {
            return Ok(None);
        }

        Ok(Some(entry.data))
    }

    /// Store component metadata in cache
    pub async fn store_component(
        &self,
        id: &str,
        metadata: &ComponentMetadata,
    ) -> RegistryResult<()> {
        let entry =
            CacheEntry { data: metadata.clone(), expires_at: self.current_timestamp() + self.ttl };

        let cache_path = self.cache_dir.join(format!("component_{}.json", id));
        let content = serde_json::to_string_pretty(&entry)
            .map_err(|e| RegistryError::CacheError(format!("Failed to serialize: {}", e)))?;

        fs::write(&cache_path, content)
            .await
            .map_err(|e| RegistryError::CacheError(format!("Failed to write cache: {}", e)))?;

        Ok(())
    }

    /// Get cached search results
    pub fn get_search(&self, query: &str) -> Option<Vec<ComponentMetadata>> {
        let cache = self.search_cache.read();
        if let Some(entry) = cache.get(query) {
            if !self.is_expired(entry.expires_at) {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Store search results in cache
    pub fn store_search(&self, query: String, results: Vec<ComponentMetadata>) {
        let entry = CacheEntry { data: results, expires_at: self.current_timestamp() + self.ttl };
        let mut cache = self.search_cache.write();
        cache.insert(query, entry);
    }

    /// Clear all cache
    pub async fn clear(&self) -> RegistryResult<()> {
        let _ = fs::remove_dir_all(&self.cache_dir).await;
        self.search_cache.write().clear();
        self.initialize().await?;
        Ok(())
    }

    fn is_expired(&self, expires_at: u64) -> bool {
        let now = self.current_timestamp();
        now > expires_at
    }

    fn current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_secs()
    }
}
