use agent_ports::{HealthStatus, PortError, PortResult, ToolManifest, ToolProvider};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// 工具提供者发现引擎
pub struct ToolProviderDiscovery {
    providers: RwLock<HashMap<String, Arc<dyn ToolProvider>>>,
}

impl ToolProviderDiscovery {
    pub fn new(_governance_root: Option<&Path>) -> Self {
        Self { providers: RwLock::new(HashMap::new()) }
    }

    /// 添加提供者
    pub async fn add_provider(&self, provider: Arc<dyn ToolProvider>) {
        let name = provider.provider_name().to_string();
        let provider_type = provider.provider_type();
        self.providers.write().await.insert(name.clone(), provider);
        info!("Added tool provider: {} ({:?})", name, provider_type);
    }

    /// 移除提供者
    pub async fn remove_provider(&self, name: &str) -> Option<Arc<dyn ToolProvider>> {
        let provider = self.providers.write().await.remove(name);
        if let Some(ref p) = provider {
            info!("Removed tool provider: {}", p.provider_name());
        }
        provider
    }

    /// 获取所有提供者的工具清单
    pub async fn all_manifests(&self) -> PortResult<Vec<ToolManifest>> {
        let providers = self.providers.read().await;
        let mut all_manifests = Vec::new();

        for (name, provider) in providers.iter() {
            match provider.list_tools().await {
                Ok(manifests) => {
                    debug!("Provider {} returned {} tools", name, manifests.len());
                    all_manifests.extend(manifests);
                }
                Err(e) => {
                    error!("Failed to list tools from provider {}: {}", name, e);
                    // 继续处理其他提供者
                }
            }
        }

        Ok(all_manifests)
    }

    /// 按名称查找工具提供者
    pub async fn get_provider(&self, name: &str) -> Option<Arc<dyn ToolProvider>> {
        self.providers.read().await.get(name).cloned()
    }

    /// 查找工具所在的提供者
    pub async fn find_provider_for_tool(&self, tool_name: &str) -> Option<Arc<dyn ToolProvider>> {
        let providers = self.providers.read().await;

        for provider in providers.values() {
            if let Ok(manifests) = provider.list_tools().await {
                if manifests.iter().any(|m| m.name == tool_name) {
                    return Some(provider.clone());
                }
            }
        }

        None
    }

    /// 获取所有提供者名称
    pub async fn list_providers(&self) -> Vec<String> {
        self.providers.read().await.keys().cloned().collect()
    }

    /// 获取提供者数量
    pub async fn provider_count(&self) -> usize {
        self.providers.read().await.len()
    }

    /// 健康检查所有提供者
    pub async fn health_check_all(&self) -> HashMap<String, HealthStatus> {
        let providers = self.providers.read().await;
        let mut results = HashMap::new();

        for (name, provider) in providers.iter() {
            let status = provider
                .health_check()
                .await
                .unwrap_or_else(|e| HealthStatus::Unhealthy(format!("Health check failed: {}", e)));
            results.insert(name.clone(), status);
        }

        results
    }
}

/// 从配置文件加载 MCP 提供者
pub async fn load_mcp_providers(
    discovery: &ToolProviderDiscovery,
    mcp_config_path: &Path,
) -> PortResult<()> {
    use crate::mcp_provider::McpToolProvider;
    use mcp_client::load_mcp_servers_from_file;

    if !mcp_config_path.exists() {
        debug!("MCP config not found at {}", mcp_config_path.display());
        return Ok(());
    }

    let configs = load_mcp_servers_from_file(
        mcp_config_path
            .to_str()
            .ok_or_else(|| PortError::Tool("MCP config path is not valid UTF-8".to_string()))?,
    )
    .await
    .map_err(|e| PortError::Tool(format!("Failed to load MCP config: {}", e)))?;

    let enabled_configs: Vec<_> = configs.into_iter().filter(|c| c.enabled).collect();

    if enabled_configs.is_empty() {
        debug!("No enabled MCP servers in config");
        return Ok(());
    }

    let provider = McpToolProvider::new_multi(enabled_configs)
        .await
        .map_err(|e| PortError::Tool(format!("Failed to create MCP provider: {}", e)))?;

    discovery.add_provider(Arc::new(provider)).await;
    Ok(())
}

/// 从配置加载 Skill 提供者
pub async fn load_skill_provider(
    discovery: &ToolProviderDiscovery,
    skills_root: &Path,
) -> PortResult<()> {
    use crate::skill_provider::SkillToolProvider;

    if !skills_root.exists() {
        debug!("Skills root not found at {}", skills_root.display());
        return Ok(());
    }

    let provider = SkillToolProvider::new(skills_root.to_string_lossy().to_string())
        .map_err(|e| PortError::Skill(format!("Failed to create skill provider: {}", e)))?;

    discovery.add_provider(Arc::new(provider)).await;
    Ok(())
}
