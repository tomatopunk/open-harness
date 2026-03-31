use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// MCP Bridge 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpBridgeConfig {
    /// MCP 服务器配置文件路径
    #[serde(default = "default_mcp_config_path")]
    pub mcp_config_path: PathBuf,

    /// 技能根目录
    #[serde(default = "default_skills_root")]
    pub skills_root: PathBuf,

    /// 是否启用技能 MCP
    #[serde(default = "default_true")]
    pub enable_skill_mcp: bool,

    /// 服务器配置列表
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl Default for McpBridgeConfig {
    fn default() -> Self {
        Self {
            mcp_config_path: default_mcp_config_path(),
            skills_root: default_skills_root(),
            enable_skill_mcp: true,
            servers: Vec::new(),
        }
    }
}

fn default_mcp_config_path() -> PathBuf {
    PathBuf::from("mcp-servers.json")
}

fn default_skills_root() -> PathBuf {
    PathBuf::from("skills")
}

fn default_true() -> bool {
    true
}

/// MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 服务器名称
    pub name: String,

    /// 服务器命令
    pub command: String,

    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,

    /// 环境变量
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// 是否是技能 MCP 服务器
    #[serde(default)]
    pub is_skill_mcp: bool,

    /// 相关技能名称（如果是技能 MCP）
    #[serde(default)]
    pub skill_names: Vec<String>,
}

impl McpServerConfig {
    /// 服务器 ID
    pub fn id(&self) -> String {
        self.name.clone()
    }
}

impl McpBridgeConfig {
    /// 从文件加载配置
    pub fn from_file(path: &PathBuf) -> crate::McpBridgeResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&content)
            .map_err(|e| crate::McpBridgeError::Config(format!("Failed to parse config: {}", e)))?;

        Ok(config)
    }

    /// 保存配置到文件
    pub fn save_to_file(&self, path: &PathBuf) -> crate::McpBridgeResult<()> {
        let content = serde_yaml::to_string(self).map_err(|e| {
            crate::McpBridgeError::Config(format!("Failed to serialize config: {}", e))
        })?;

        std::fs::write(path, content)?;
        Ok(())
    }

    /// 获取启用的服务器
    pub fn enabled_servers(&self) -> Vec<&McpServerConfig> {
        self.servers.iter().filter(|s| s.enabled).collect()
    }

    /// 添加服务器
    pub fn add_server(&mut self, server: McpServerConfig) {
        self.servers.push(server);
    }

    /// 移除服务器
    pub fn remove_server(&mut self, name: &str) -> Option<McpServerConfig> {
        let index = self.servers.iter().position(|s| s.name == name)?;
        Some(self.servers.remove(index))
    }
}
