use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 插件清单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件名称
    pub name: String,

    /// 插件版本
    pub version: String,

    /// 插件描述
    pub description: String,

    /// 插件作者
    pub authors: Vec<String>,

    /// 插件类型
    #[serde(rename = "type")]
    pub plugin_type: PluginType,

    /// 入口点（对于动态加载插件）
    #[serde(default)]
    pub entry_point: Option<String>,

    /// 依赖的其他插件
    #[serde(default)]
    pub dependencies: Vec<String>,

    /// 配置模式
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,

    /// 是否默认启用
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 插件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    /// API 插件
    Api,
    /// 工具插件
    Tool,
    /// 模型插件
    Model,
    /// 存储插件
    Storage,
    /// 渠道插件
    Channel,
    /// 通用插件
    Generic,
}

impl PluginManifest {
    /// 从文件加载插件清单
    pub fn from_file(path: &PathBuf) -> crate::PluginResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = serde_yaml::from_str(&content).map_err(|e| {
            crate::PluginError::InvalidManifest(format!("Failed to parse manifest: {}", e))
        })?;
        Ok(manifest)
    }

    /// 插件 ID（name@version）
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}
