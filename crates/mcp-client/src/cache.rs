use crate::types::{HealthStatus, McpServerConfig, McpTool};
use crate::McpResult;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

/// MCP 工具缓存管理器
pub struct McpToolsCache {
    servers: Arc<RwLock<Vec<McpServerConfig>>>,
    cache: RwLock<Option<Vec<McpTool>>>,
    config_path: Option<String>,
    config_mtime: RwLock<Option<u64>>,
}

impl McpToolsCache {
    pub fn new(servers: Vec<McpServerConfig>, config_path: Option<String>) -> Self {
        Self {
            servers: Arc::new(RwLock::new(servers)),
            cache: RwLock::new(None),
            config_path,
            config_mtime: RwLock::new(None),
        }
    }

    /// 获取缓存的工具，如果缓存失效则重新加载
    pub async fn get_tools(&self) -> McpResult<Vec<McpTool>> {
        // 检查缓存是否失效
        if self.is_cache_stale().await {
            tracing::info!("MCP config file has been modified, cache is stale");
            self.reset_cache().await;
        }

        // 懒加载
        let mut cache_guard = self.cache.write().await;
        if cache_guard.is_none() {
            let tools = self.load_tools().await?;
            *cache_guard = Some(tools);

            // 记录配置 mtime
            if let Some(path) = &self.config_path {
                if let Ok(meta) = fs::metadata(path).await {
                    if let Ok(modified) = meta.modified() {
                        *self.config_mtime.write().await =
                            Some(modified.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
                    }
                }
            }

            tracing::info!("Loaded {} MCP tools", cache_guard.as_ref().unwrap().len());
        }

        Ok(cache_guard.as_ref().unwrap().clone())
    }

    /// 重置缓存
    pub async fn reset_cache(&self) {
        *self.cache.write().await = None;
        *self.config_mtime.write().await = None;
        tracing::debug!("MCP tools cache reset");
    }

    /// 检查缓存是否失效
    async fn is_cache_stale(&self) -> bool {
        let Some(config_path) = &self.config_path else {
            return false;
        };

        let Some(old_mtime) = *self.config_mtime.read().await else {
            return false;
        };

        if let Ok(meta) = fs::metadata(config_path).await {
            if let Ok(new_mtime) = meta.modified() {
                let new_mtime_secs = new_mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("Time should be valid")
                    .as_secs();
                return new_mtime_secs > old_mtime;
            }
        }

        false
    }

    /// 从所有启用的服务器加载工具
    async fn load_tools(&self) -> McpResult<Vec<McpTool>> {
        let servers = self.servers.read().await;
        let all_tools = Vec::new();

        for server_config in servers.iter() {
            if !server_config.enabled {
                continue;
            }

            // 这里需要实际的 MCP 客户端实现来获取工具
            // 目前先返回空列表，后续实现具体的 MCP 协议客户端
            tracing::debug!("Would load tools from MCP server: {}", server_config.name);
        }

        Ok(all_tools)
    }

    /// 更新服务器配置
    pub async fn update_servers(&self, servers: Vec<McpServerConfig>) {
        *self.servers.write().await = servers;
        self.reset_cache().await;
    }

    /// 获取健康状态
    pub async fn health_check(&self) -> HealthStatus {
        match self.get_tools().await {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => HealthStatus::Unhealthy(e.to_string()),
        }
    }
}

/// 从文件加载 MCP 服务器配置
pub async fn load_mcp_servers_from_file(config_path: &str) -> McpResult<Vec<McpServerConfig>> {
    let content = fs::read_to_string(config_path)
        .await
        .map_err(|e| crate::McpError::ConnectionFailed(format!("Failed to read config: {}", e)))?;

    let config: McpConfigFile = serde_yaml::from_str(&content)
        .map_err(|e| crate::McpError::Protocol(format!("Failed to parse config: {}", e)))?;

    Ok(config.servers)
}

#[derive(Debug, serde::Deserialize)]
struct McpConfigFile {
    servers: Vec<McpServerConfig>,
}
