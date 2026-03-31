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
    server_states: RwLock<HashMap<String, ServerState>>,
    tool_cache: RwLock<Option<Vec<ToolManifest>>>,
}

/// 服务器状态
#[derive(Debug, Clone)]
struct ServerState {
    config: McpServerConfig,
    connected: bool,
    tools: Vec<ToolManifest>,
}

impl McpBridgeManager {
    /// 创建新的 MCP Bridge 管理器
    pub fn new(config: McpBridgeConfig) -> Self {
        Self {
            skill_mcp_manager: SkillMcpManager::new(config.skills_root.clone()),
            server_states: RwLock::new(HashMap::new()),
            tool_cache: RwLock::new(None),
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

        let state = ServerState { config: config.clone(), connected: false, tools: Vec::new() };

        let mut states = self.server_states.write().await;
        states.insert(config.id(), state);

        // 清除工具缓存
        *self.tool_cache.write().await = None;

        Ok(())
    }

    /// 连接到所有 MCP 服务器
    pub async fn connect_all(&self) -> McpBridgeResult<()> {
        info!("Connecting to all MCP servers...");

        let states = self.server_states.read().await;
        for (id, state) in states.iter() {
            if state.config.enabled && !state.connected {
                debug!("Connecting to MCP server: {}", id);
                // 在真实实现中，这里会连接到 MCP 服务器
                // self.connect_server(id).await?;
            }
        }

        Ok(())
    }

    /// 发现所有工具
    pub async fn discover_tools(&self) -> McpBridgeResult<Vec<ToolManifest>> {
        debug!("Discovering tools from MCP servers...");

        let mut all_tools = Vec::new();
        let states = self.server_states.read().await;

        for (id, state) in states.iter() {
            if state.config.enabled && state.connected {
                debug!("Getting tools from server: {}", id);
                all_tools.extend(state.tools.clone());
            }
        }

        // 更新缓存
        *self.tool_cache.write().await = Some(all_tools.clone());

        Ok(all_tools)
    }

    /// 获取缓存的工具列表
    pub async fn cached_tools(&self) -> Option<Vec<ToolManifest>> {
        self.tool_cache.read().await.clone()
    }

    /// 列出所有已注册的服务器
    pub async fn list_servers(&self) -> Vec<McpServerConfig> {
        let states = self.server_states.read().await;
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

        let mut states = self.server_states.write().await;
        if states.remove(name).is_some() {
            *self.tool_cache.write().await = None;
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
}
