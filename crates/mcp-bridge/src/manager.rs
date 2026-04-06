use crate::config::{McpBridgeConfig, McpServerConfig};
use crate::error::{McpBridgeError, McpBridgeResult};
use crate::skill_mcp::{SkillInfo, SkillMcpManager};
use crate::types::ToolManifest;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// MCP Bridge 管理器
///
/// 统一管理所有 MCP 相关功能，包括：
/// - MCP 服务器生命周期
/// - 技能 MCP 管理
/// - 工具发现和聚合
pub struct McpBridgeManager {
    config: McpBridgeConfig,
    skill_mcp_manager: SkillMcpManager,
    server_registry: RwLock<HashMap<String, ServerState>>,
    tool_cache: RwLock<ToolCacheState>,
}

/// 服务器状态
#[derive(Debug, Clone)]
struct ServerState {
    config: McpServerConfig,
    connected: bool,
}

#[derive(Debug, Default, Clone)]
struct ToolCacheState {
    server_tools: HashMap<String, Vec<ToolManifest>>,
    aggregated: Option<Vec<ToolManifest>>,
}

impl McpBridgeManager {
    /// 创建新的 MCP Bridge 管理器
    pub fn new(config: McpBridgeConfig) -> Self {
        Self {
            skill_mcp_manager: SkillMcpManager::new(config.skills_root.clone()),
            server_registry: RwLock::new(HashMap::new()),
            tool_cache: RwLock::new(ToolCacheState::default()),
            config,
        }
    }

    /// 初始化 MCP Bridge
    pub async fn initialize(&self) -> McpBridgeResult<()> {
        info!("Initializing MCP Bridge...");

        // 加载配置的 MCP 服务器
        for server_config in self.config.enabled_servers() {
            self.register_server(server_config.clone()).await?;
        }

        // 如果启用技能 MCP，发现技能
        if self.config.enable_skill_mcp {
            let skills = self.skill_mcp_manager.discover_skills().await?;
            for skill in skills {
                if skill.mcp_command.is_some() {
                    let server_config = self.skill_mcp_manager.load_skill_mcp(&skill).await?;
                    self.register_server(server_config).await?;
                }
            }
        }

        info!("MCP Bridge initialized");
        Ok(())
    }

    /// 注册 MCP 服务器
    async fn register_server(&self, config: McpServerConfig) -> McpBridgeResult<()> {
        info!("Registering MCP server: {}", config.name);

        let server_id = config.id();
        let state = ServerState { config: config.clone(), connected: false };

        let mut states = self.server_registry.write().await;
        states.insert(server_id, state);

        Ok(())
    }

    /// 连接到所有 MCP 服务器
    pub async fn connect_all(&self) -> McpBridgeResult<()> {
        info!("Connecting to all MCP servers...");

        let server_ids = {
            let states = self.server_registry.read().await;
            states
                .iter()
                .filter(|(_, state)| state.config.enabled && !state.connected)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        for server_id in server_ids {
            self.connect_server(&server_id).await?;
        }

        Ok(())
    }

    pub async fn connect_server(&self, server_id: &str) -> McpBridgeResult<()> {
        debug!("Connecting to MCP server: {}", server_id);

        let mut states = self.server_registry.write().await;
        let state = states
            .get_mut(server_id)
            .ok_or_else(|| McpBridgeError::NotFound(server_id.to_string()))?;

        state.connected = true;
        drop(states);

        self.invalidate_tool_cache().await;
        Ok(())
    }

    pub async fn disconnect_all(&self) -> McpBridgeResult<()> {
        info!("Disconnecting all MCP servers...");

        let server_ids = {
            let states = self.server_registry.read().await;
            states
                .iter()
                .filter(|(_, state)| state.connected)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        for server_id in server_ids {
            self.disconnect_server(&server_id).await?;
        }

        Ok(())
    }

    pub async fn disconnect_server(&self, server_id: &str) -> McpBridgeResult<()> {
        debug!("Disconnecting MCP server: {}", server_id);

        let mut states = self.server_registry.write().await;
        let state = states
            .get_mut(server_id)
            .ok_or_else(|| McpBridgeError::NotFound(server_id.to_string()))?;

        state.connected = false;
        drop(states);

        self.clear_server_tools(server_id).await;
        Ok(())
    }

    /// 发现所有工具
    pub async fn discover_tools(&self) -> McpBridgeResult<Vec<ToolManifest>> {
        debug!("Discovering tools from MCP servers...");

        {
            let cache = self.tool_cache.read().await;
            if let Some(cached_tools) = &cache.aggregated {
                return Ok(cached_tools.clone());
            }
        }

        let connected_server_ids = {
            let states = self.server_registry.read().await;
            states
                .iter()
                .filter(|(_, state)| state.config.enabled && state.connected)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        let mut cache = self.tool_cache.write().await;
        let mut all_tools = Vec::new();

        for server_id in connected_server_ids {
            debug!("Getting tools from server: {}", server_id);
            if let Some(server_tools) = cache.server_tools.get(&server_id) {
                all_tools.extend(server_tools.clone());
            }
        }

        cache.aggregated = Some(all_tools.clone());

        Ok(all_tools)
    }

    /// 获取缓存的工具列表
    pub async fn cached_tools(&self) -> Option<Vec<ToolManifest>> {
        self.tool_cache.read().await.aggregated.clone()
    }

    /// 列出所有已注册的服务器
    pub async fn list_servers(&self) -> Vec<McpServerConfig> {
        let states = self.server_registry.read().await;
        states.values().map(|s| s.config.clone()).collect()
    }

    /// 动态添加服务器
    pub async fn add_server(&self, config: McpServerConfig) -> McpBridgeResult<()> {
        info!("Adding MCP server dynamically: {}", config.name);
        self.register_server(config).await
    }

    /// 动态移除服务器
    pub async fn remove_server(&self, name: &str) -> McpBridgeResult<()> {
        info!("Removing MCP server: {}", name);

        let mut states = self.server_registry.write().await;
        if states.remove(name).is_some() {
            drop(states);
            self.clear_server_tools(name).await;
            Ok(())
        } else {
            Err(McpBridgeError::NotFound(name.to_string()))
        }
    }

    /// 发现技能
    pub async fn discover_skills(&self) -> McpBridgeResult<Vec<SkillInfo>> {
        self.skill_mcp_manager.discover_skills().await
    }

    /// 加载技能 MCP
    pub async fn load_skill(&self, skill: &SkillInfo) -> McpBridgeResult<()> {
        let server_config = self.skill_mcp_manager.load_skill_mcp(skill).await?;
        self.register_server(server_config).await?;
        Ok(())
    }

    /// 卸载技能 MCP
    pub async fn unload_skill(&self, skill_name: &str) -> McpBridgeResult<()> {
        self.skill_mcp_manager.unload_skill_mcp(skill_name).await?;
        let server_id = format!("skill-{}", skill_name);
        self.remove_server(&server_id).await?;
        Ok(())
    }

    async fn invalidate_tool_cache(&self) {
        let mut cache = self.tool_cache.write().await;
        cache.aggregated = None;
    }

    async fn clear_server_tools(&self, server_id: &str) {
        let mut cache = self.tool_cache.write().await;
        cache.server_tools.remove(server_id);
        cache.aggregated = None;
    }

    #[cfg(test)]
    async fn update_server_tools(
        &self,
        server_id: &str,
        tools: Vec<ToolManifest>,
    ) -> McpBridgeResult<()> {
        let is_connected = {
            let states = self.server_registry.read().await;
            let state = states
                .get(server_id)
                .ok_or_else(|| McpBridgeError::NotFound(server_id.to_string()))?;
            state.connected && state.config.enabled
        };

        let mut cache = self.tool_cache.write().await;
        if is_connected {
            cache.server_tools.insert(server_id.to_string(), tools);
        } else {
            cache.server_tools.remove(server_id);
        }
        cache.aggregated = None;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RiskLevel, SideEffectClass, ToolProviderType};
    use serde_json::json;

    fn test_server(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            transport: "stdio".to_string(),
            command: "test-mcp".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            enabled: true,
            is_skill_mcp: false,
            skill_names: Vec::new(),
            description: String::new(),
        }
    }

    fn test_tool(name: &str, provider_name: &str) -> ToolManifest {
        ToolManifest {
            name: name.to_string(),
            description: Some(format!("tool {name}")),
            input_schema: Some(json!({"type": "object"})),
            capability_tags: vec!["test".to_string()],
            risk_level: RiskLevel::Low,
            timeout_ms: 1_000,
            retry_max: 0,
            side_effect_class: SideEffectClass::Read,
            provider_type: ToolProviderType::Mcp,
            provider_name: provider_name.to_string(),
            load_path: None,
            version: Some("0.1.0".to_string()),
        }
    }

    #[tokio::test]
    async fn server_lifecycle_updates_cached_tools() {
        let manager = McpBridgeManager::new(McpBridgeConfig::default());
        manager.add_server(test_server("alpha")).await.unwrap();

        manager.connect_all().await.unwrap();
        manager.update_server_tools("alpha", vec![test_tool("list-files", "alpha")]).await.unwrap();

        let discovered = manager.discover_tools().await.unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(manager.cached_tools().await.unwrap().len(), 1);

        manager.disconnect_all().await.unwrap();
        assert!(manager.cached_tools().await.is_none());

        let discovered_after_disconnect = manager.discover_tools().await.unwrap();
        assert!(discovered_after_disconnect.is_empty());
        assert!(manager.cached_tools().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reconnect_does_not_reuse_stale_tool_cache() {
        let manager = McpBridgeManager::new(McpBridgeConfig::default());
        manager.add_server(test_server("alpha")).await.unwrap();

        manager.connect_server("alpha").await.unwrap();
        manager.update_server_tools("alpha", vec![test_tool("stale-tool", "alpha")]).await.unwrap();

        let first_discovery = manager.discover_tools().await.unwrap();
        assert_eq!(
            first_discovery.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
            vec!["stale-tool"]
        );

        manager.disconnect_server("alpha").await.unwrap();
        assert!(manager.cached_tools().await.is_none());

        manager.connect_server("alpha").await.unwrap();
        manager.update_server_tools("alpha", vec![test_tool("fresh-tool", "alpha")]).await.unwrap();

        let second_discovery = manager.discover_tools().await.unwrap();
        assert_eq!(
            second_discovery.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
            vec!["fresh-tool"]
        );
        assert!(!second_discovery.iter().any(|tool| tool.name == "stale-tool"));
    }
}
