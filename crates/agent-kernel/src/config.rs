use llm_providers::{ProviderConfig, ProviderType};
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

    /// 日志级别
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// LLM provider 配置
    #[serde(default)]
    pub llm: ProviderConfig,

    /// Agent Loop 配置
    #[serde(default)]
    pub agent_loop: AgentLoopConfig,

    /// Memory 系统配置
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Channels 配置
    #[serde(default)]
    pub channels: ChannelsConfig,

    /// Storage 配置
    #[serde(default)]
    pub storage: StorageConfig,

    /// Extensions 配置路径
    #[serde(default)]
    pub extensions_config_path: Option<String>,

    /// Gateway 配置
    #[serde(default)]
    pub gateway: Option<GatewayConfig>,

    /// Manage 配置
    #[serde(default)]
    pub manage: Option<ManageConfig>,
}

/// Agent Loop 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLoopConfig {
    /// 是否启用
    pub enabled: bool,
    /// 最大迭代次数
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// 完成承诺关键词
    #[serde(default = "default_completion_promise")]
    pub completion_promise: String,
    /// 去抖时间（秒）
    #[serde(default = "default_debounce_seconds")]
    pub debounce_seconds: u64,
}

impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: default_max_iterations(),
            completion_promise: default_completion_promise(),
            debounce_seconds: default_debounce_seconds(),
        }
    }
}

fn default_max_iterations() -> usize {
    100
}
fn default_completion_promise() -> String {
    "DONE".to_string()
}
fn default_debounce_seconds() -> u64 {
    30
}

/// Memory 系统配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// 是否启用
    pub enabled: bool,
    /// 存储路径（相对 local_fs root）
    #[serde(default)]
    pub storage_path: String,
    /// 最大事实数量
    #[serde(default = "default_max_facts")]
    pub max_facts: usize,
    /// 事实置信度阈值
    #[serde(default = "default_fact_threshold")]
    pub fact_confidence_threshold: f32,
    /// 是否启用注入
    #[serde(default = "default_injection_enabled")]
    pub injection_enabled: bool,
    /// 最大注入 tokens
    #[serde(default = "default_max_injection_tokens")]
    pub max_injection_tokens: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage_path: "memory/memory.json".to_string(),
            max_facts: default_max_facts(),
            fact_confidence_threshold: default_fact_threshold(),
            injection_enabled: default_injection_enabled(),
            max_injection_tokens: default_max_injection_tokens(),
        }
    }
}

fn default_max_facts() -> usize {
    100
}
fn default_fact_threshold() -> f32 {
    0.7
}
fn default_injection_enabled() -> bool {
    true
}
fn default_max_injection_tokens() -> usize {
    2000
}

/// Channels 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    /// 启用的渠道列表
    #[serde(default)]
    pub enabled: Vec<String>,
    /// 钉钉配置
    pub dingtalk: Option<DingtalkConfig>,
    /// 企业微信配置
    pub wecom: Option<WecomConfig>,
    /// 监听地址
    #[serde(default = "default_channel_bind")]
    pub bind: String,
}

fn default_channel_bind() -> String {
    "0.0.0.0:8082".to_string()
}

/// 钉钉配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingtalkConfig {
    pub enabled: bool,
    pub webhook_url: Option<String>,
    pub app_secret: Option<String>,
}

/// 企业微信配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WecomConfig {
    pub enabled: bool,
    pub webhook_url: Option<String>,
}

/// Storage 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "mode")]
    pub mode: StorageMode,
    pub local_fs: Option<LocalFsConfig>,
    pub sqlite: Option<SqliteConfig>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            mode: StorageMode::LocalFs,
            local_fs: Some(LocalFsConfig { root: PathBuf::from(".deer-flow/local-fs") }),
            sqlite: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    LocalFs,
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalFsConfig {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    pub url: String,
}

/// Gateway 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_bind")]
    pub bind: String,
}

fn default_gateway_bind() -> String {
    "0.0.0.0:8080".to_string()
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self { bind: default_gateway_bind() }
    }
}

/// Manage 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManageConfig {
    #[serde(default = "default_manage_bind")]
    pub bind: String,
}

fn default_manage_bind() -> String {
    "0.0.0.0:8081".to_string()
}

impl Default for ManageConfig {
    fn default() -> Self {
        Self { bind: default_manage_bind() }
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            workspace_root: default_workspace_root(),
            plugins_dir: default_plugins_dir(),
            log_level: default_log_level(),
            llm: ProviderConfig::new(ProviderType::Rig, "gpt-4"),
            agent_loop: AgentLoopConfig::default(),
            memory: MemoryConfig::default(),
            channels: ChannelsConfig::default(),
            storage: StorageConfig::default(),
            extensions_config_path: None,
            gateway: Some(GatewayConfig::default()),
            manage: Some(ManageConfig::default()),
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
