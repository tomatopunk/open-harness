use crate::{McpBridgeError, McpBridgeResult, McpServerConfig};
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// 技能 MCP 管理器
///
/// 管理技能相关的 MCP 服务器，支持动态加载/卸载
pub struct SkillMcpManager {
    skills_root: PathBuf,
    active_servers: RwLock<std::collections::HashMap<String, McpServerConfig>>,
}

impl SkillMcpManager {
    /// 创建新的技能 MCP 管理器
    pub fn new(skills_root: PathBuf) -> Self {
        Self { skills_root, active_servers: RwLock::new(std::collections::HashMap::new()) }
    }

    /// 发现可用技能
    pub async fn discover_skills(&self) -> McpBridgeResult<Vec<SkillInfo>> {
        info!("Discovering skills in: {:?}", self.skills_root);

        let mut skills = Vec::new();

        if !self.skills_root.exists() {
            debug!("Skills root does not exist");
            return Ok(skills);
        }

        let mut entries = tokio::fs::read_dir(&self.skills_root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                let skill_yaml = path.join("skill.yaml");
                if skill_yaml.exists() {
                    match SkillInfo::from_file(&skill_yaml) {
                        Ok(skill) => {
                            info!("Discovered skill: {}", skill.name);
                            skills.push(skill);
                        }
                        Err(e) => {
                            debug!("Failed to load skill from {:?}: {}", skill_yaml, e);
                        }
                    }
                }
            }
        }

        Ok(skills)
    }

    /// 加载技能 MCP 服务器
    pub async fn load_skill_mcp(&self, skill: &SkillInfo) -> McpBridgeResult<McpServerConfig> {
        info!("Loading skill MCP: {}", skill.name);

        let server_id = format!("skill-{}", skill.name);

        // 检查是否已加载
        {
            let active = self.active_servers.read().await;
            if active.contains_key(&server_id) {
                return Err(McpBridgeError::AlreadyExists(server_id));
            }
        }

        // 创建 MCP 服务器配置
        let server_config = McpServerConfig {
            name: server_id.clone(),
            command: skill.mcp_command.clone().unwrap_or_else(|| "mcp-server".to_string()),
            args: skill.mcp_args.clone().unwrap_or_default(),
            env: skill.mcp_env.clone().unwrap_or_default(),
            enabled: true,
            is_skill_mcp: true,
            skill_names: vec![skill.name.clone()],
        };

        // 注册到活动服务器
        {
            let mut active = self.active_servers.write().await;
            active.insert(server_id.clone(), server_config.clone());
        }

        info!("Skill MCP loaded: {}", server_id);
        Ok(server_config)
    }

    /// 卸载技能 MCP 服务器
    pub async fn unload_skill_mcp(&self, skill_name: &str) -> McpBridgeResult<()> {
        let server_id = format!("skill-{}", skill_name);

        info!("Unloading skill MCP: {}", server_id);

        let removed = {
            let mut active = self.active_servers.write().await;
            active.remove(&server_id)
        };

        if removed.is_some() {
            info!("Skill MCP unloaded: {}", server_id);
            Ok(())
        } else {
            Err(McpBridgeError::NotFound(server_id))
        }
    }

    /// 获取活动的技能 MCP 服务器
    pub async fn active_skill_mcps(&self) -> Vec<McpServerConfig> {
        let active = self.active_servers.read().await;
        active.values().cloned().collect()
    }
}

/// 技能信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillInfo {
    /// 技能名称
    pub name: String,

    /// 技能版本
    pub version: String,

    /// 技能描述
    pub description: String,

    /// 技能作者
    #[serde(default)]
    pub authors: Vec<String>,

    /// MCP 服务器命令（可选）
    #[serde(rename = "mcpCommand")]
    pub mcp_command: Option<String>,

    /// MCP 服务器参数（可选）
    #[serde(rename = "mcpArgs")]
    pub mcp_args: Option<Vec<String>>,

    /// MCP 环境变量（可选）
    #[serde(rename = "mcpEnv")]
    pub mcp_env: Option<std::collections::HashMap<String, String>>,
}

impl SkillInfo {
    /// 从文件加载技能信息
    pub fn from_file(path: &PathBuf) -> McpBridgeResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let skill: Self = serde_yaml::from_str(&content)
            .map_err(|e| McpBridgeError::Config(format!("Failed to parse skill: {}", e)))?;
        Ok(skill)
    }
}
