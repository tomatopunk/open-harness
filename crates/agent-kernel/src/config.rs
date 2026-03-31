use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// 工作目录
    #[serde(default = "default_workspace_root")]
    pub workspace_root: PathBuf,

    /// 插件目录
    #[serde(default = "default_plugins_dir")]
    pub plugins_dir: PathBuf,

    /// 是否启用开发模式
    #[serde(default)]
    pub dev_mode: bool,

    /// 日志级别
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            workspace_root: default_workspace_root(),
            plugins_dir: default_plugins_dir(),
            dev_mode: false,
            log_level: default_log_level(),
        }
    }
}

fn default_workspace_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_plugins_dir() -> PathBuf {
    PathBuf::from("plugins")
}

fn default_log_level() -> String {
    "info".to_string()
}

impl KernelConfig {
    /// 从文件加载配置
    pub fn from_file(path: &PathBuf) -> crate::KernelResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&content)
            .map_err(|e| crate::KernelError::Config(format!("Failed to parse config: {}", e)))?;

        Ok(config)
    }
}
