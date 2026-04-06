use llm_providers::{ProviderConfig, ProviderType};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use unified_config::loader::{AppConfigRef, ModelConfigRef};
use unified_config::UnifiedConfig;

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

    #[serde(default)]
    pub mcp: mcp_bridge::McpBridgeConfig,

    /// Extensions 配置路径
    #[serde(default)]
    pub extensions_config_path: Option<String>,

    #[serde(default)]
    pub migration: KernelMigrationConfig,

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
            local_fs: Some(LocalFsConfig { root: PathBuf::from(".data/local-fs") }),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KernelRuntimeMode {
    #[default]
    Legacy,
    Unified,
    Auto,
}

impl fmt::Display for KernelRuntimeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Legacy => "legacy",
            Self::Unified => "unified",
            Self::Auto => "auto",
        };

        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelMigrationConfig {
    #[serde(default)]
    pub mode: KernelRuntimeMode,
    #[serde(default = "default_governance_root")]
    pub governance_root: PathBuf,
    #[serde(default = "default_true")]
    pub rollback_on_unified_failure: bool,
}

impl Default for KernelMigrationConfig {
    fn default() -> Self {
        Self {
            mode: KernelRuntimeMode::Legacy,
            governance_root: default_governance_root(),
            rollback_on_unified_failure: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelConfigResolution {
    pub requested_mode: KernelRuntimeMode,
    pub effective_mode: KernelRuntimeMode,
    pub rollback_reason: Option<String>,
}

impl KernelConfigResolution {
    fn legacy(requested_mode: KernelRuntimeMode) -> Self {
        Self { requested_mode, effective_mode: KernelRuntimeMode::Legacy, rollback_reason: None }
    }

    fn unified(requested_mode: KernelRuntimeMode) -> Self {
        Self { requested_mode, effective_mode: KernelRuntimeMode::Unified, rollback_reason: None }
    }

    fn auto_rollback(reason: impl Into<String>) -> Self {
        Self {
            requested_mode: KernelRuntimeMode::Auto,
            effective_mode: KernelRuntimeMode::Legacy,
            rollback_reason: Some(reason.into()),
        }
    }

    pub fn rolled_back(&self) -> bool {
        self.requested_mode != self.effective_mode
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedKernelConfig {
    pub config: KernelConfig,
    pub resolution: KernelConfigResolution,
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
            mcp: mcp_bridge::McpBridgeConfig::default(),
            extensions_config_path: None,
            migration: KernelMigrationConfig::default(),
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

fn default_governance_root() -> PathBuf {
    PathBuf::from("governance")
}

fn default_true() -> bool {
    true
}

impl KernelConfig {
    pub fn resolve_runtime(path: &PathBuf) -> crate::KernelResult<ResolvedKernelConfig> {
        let base_config = Self::from_file(path)?;
        let requested_mode = runtime_mode_override_from_env().unwrap_or(base_config.migration.mode);

        match requested_mode {
            KernelRuntimeMode::Legacy => Ok(ResolvedKernelConfig {
                config: base_config,
                resolution: KernelConfigResolution::legacy(requested_mode),
            }),
            KernelRuntimeMode::Unified => {
                let config = base_config.resolve_unified_runtime()?;
                Ok(ResolvedKernelConfig {
                    config,
                    resolution: KernelConfigResolution::unified(requested_mode),
                })
            }
            KernelRuntimeMode::Auto => match base_config.resolve_unified_runtime() {
                Ok(config) => Ok(ResolvedKernelConfig {
                    config,
                    resolution: KernelConfigResolution::unified(requested_mode),
                }),
                Err(error) if base_config.migration.rollback_on_unified_failure => {
                    Ok(ResolvedKernelConfig {
                        config: base_config,
                        resolution: KernelConfigResolution::auto_rollback(error.to_string()),
                    })
                }
                Err(error) => Err(error),
            },
        }
    }

    pub fn from_unified_config(config: &UnifiedConfig) -> crate::KernelResult<Self> {
        let runtime_view = config
            .kernel_runtime_view()
            .map_err(|error| crate::KernelError::Config(error.to_string()))?;
        let llm = provider_config_from_runtime_view(&runtime_view.llm)?;

        Ok(Self { llm, ..Self::default() })
    }

    fn resolve_unified_runtime(&self) -> crate::KernelResult<Self> {
        let app_config = AppConfigRef {
            models: vec![self.legacy_model_ref()],
            extensions_config_path: self.extensions_config_path.clone(),
        };
        let governance_root = self.migration.governance_root.to_string_lossy().to_string();
        let unified_config =
            unified_config::loader::load_unified_config(&app_config, &governance_root)
                .map_err(|error| crate::KernelError::Config(error.to_string()))?;
        let runtime_view = unified_config
            .kernel_runtime_view()
            .map_err(|error| crate::KernelError::Config(error.to_string()))?;
        let mut resolved = self.clone();
        resolved.llm = provider_config_from_runtime_view(&runtime_view.llm)?;
        resolved.mcp = self.resolve_mcp_bridge_config()?;
        Ok(resolved)
    }

    /// 从文件加载配置
    pub fn from_file(path: &PathBuf) -> crate::KernelResult<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&content).map_err(|error| {
            crate::KernelError::Config(format!(
                "config parse error in kernel config ({}): {}",
                path.display(),
                error
            ))
        })?;

        Ok(config)
    }

    fn legacy_model_ref(&self) -> ModelConfigRef {
        ModelConfigRef {
            name: self.llm.model.clone(),
            display_name: self.llm.model.clone(),
            use_provider: provider_type_to_unified(self.llm.provider_type).to_string(),
            model: self.llm.model.clone(),
            api_key: self.llm.api_key.clone(),
            max_tokens: self.llm.generation.max_tokens,
            temperature: self.llm.generation.temperature,
            base_url: self.llm.base_url.clone(),
            use_responses_api: self
                .llm
                .extra
                .get("use_responses_api")
                .and_then(serde_json::Value::as_bool),
            output_version: self.llm.api_version.clone(),
        }
    }

    fn resolve_mcp_bridge_config(&self) -> crate::KernelResult<mcp_bridge::McpBridgeConfig> {
        let mut mcp_config = self.mcp.clone();
        let Some(path) = self.extensions_config_path.as_deref() else {
            return Ok(mcp_config);
        };

        let path = Path::new(path);
        if !path.exists() {
            return Ok(mcp_config);
        }

        let extensions = unified_config::ExtensionsConfig::from_file(path)
            .map_err(|error| crate::KernelError::Config(error.to_string()))?;

        let mut servers: Vec<mcp_bridge::McpServerConfig> = extensions
            .mcp_servers
            .iter()
            .map(|(name, server)| mcp_bridge::McpServerConfig {
                name: name.clone(),
                transport: server.r#type.clone(),
                command: server.command.clone().unwrap_or_default(),
                args: server.args.clone(),
                env: server.env.clone(),
                url: server.url.clone(),
                enabled: server.enabled,
                is_skill_mcp: false,
                skill_names: Vec::new(),
                description: server.description.clone(),
            })
            .collect();
        servers.sort_by(|left, right| left.name.cmp(&right.name));
        mcp_config.servers = servers;

        Ok(mcp_config)
    }
}

fn runtime_mode_override_from_env() -> Option<KernelRuntimeMode> {
    let raw = env::var("OPEN_HARNESS_KERNEL_MODE").ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "legacy" => Some(KernelRuntimeMode::Legacy),
        "unified" => Some(KernelRuntimeMode::Unified),
        "auto" => Some(KernelRuntimeMode::Auto),
        _ => None,
    }
}

fn provider_type_to_unified(provider_type: ProviderType) -> &'static str {
    match provider_type {
        ProviderType::Rig => "rig",
        ProviderType::OpenAI => "open_ai",
        ProviderType::Anthropic => "anthropic",
        ProviderType::Azure => "azure",
        ProviderType::Google => "google",
        ProviderType::Bedrock => "bedrock",
    }
}

fn provider_config_from_runtime_view(
    llm: &unified_config::KernelRuntimeLlmConfig,
) -> crate::KernelResult<ProviderConfig> {
    let mut provider_config =
        ProviderConfig::new(provider_type_from_unified(&llm.provider)?, &llm.model_id);
    provider_config.api_key = llm.config.api_key.clone();
    provider_config.base_url = llm.config.base_url.clone();
    provider_config.api_version = llm.config.output_version.clone();
    provider_config.extra = llm.config.extra.clone().into_iter().collect();
    provider_config.generation.max_tokens = llm.config.max_tokens;
    provider_config.generation.temperature = llm.config.temperature;

    if let Some(use_responses_api) = llm.config.use_responses_api {
        provider_config
            .extra
            .insert("use_responses_api".to_string(), serde_json::Value::Bool(use_responses_api));
    }

    Ok(provider_config)
}

fn provider_type_from_unified(provider: &str) -> crate::KernelResult<ProviderType> {
    let normalized = provider.trim().to_ascii_lowercase();

    if normalized == "rig" {
        return Ok(ProviderType::Rig);
    }

    if normalized == "open_ai"
        || normalized == "openai"
        || normalized.contains("openai")
        || normalized.contains("open_ai")
    {
        return Ok(ProviderType::OpenAI);
    }

    if normalized.contains("anthropic") {
        return Ok(ProviderType::Anthropic);
    }

    if normalized.contains("azure") {
        return Ok(ProviderType::Azure);
    }

    if normalized.contains("google") || normalized.contains("gemini") {
        return Ok(ProviderType::Google);
    }

    if normalized.contains("bedrock") {
        return Ok(ProviderType::Bedrock);
    }

    Err(crate::KernelError::Config(format!("unsupported unified model provider '{provider}'")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_providers::ProviderType;
    use std::fs;
    use unified_config::{ModelConfig, ModelEntry, ModelRegistry};
    use uuid::Uuid;

    fn temp_test_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("agent-kernel-config-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn test_from_unified_config_maps_selected_model_into_llm_provider_config() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: "gpt-4o".to_string(),
                entries: vec![
                    ModelEntry {
                        name: "gpt-4".to_string(),
                        display_name: "GPT-4".to_string(),
                        provider: "rig".to_string(),
                        model_id: "gpt-4".to_string(),
                        config: ModelConfig::default(),
                    },
                    ModelEntry {
                        name: "gpt-4o".to_string(),
                        display_name: "GPT-4o".to_string(),
                        provider: "langchain_openai:ChatOpenAI".to_string(),
                        model_id: "gpt-4o".to_string(),
                        config: ModelConfig {
                            api_key: Some("$OPENAI_API_KEY".to_string()),
                            max_tokens: Some(8192),
                            temperature: Some(0.4),
                            base_url: Some("https://api.openai.com/v1".to_string()),
                            use_responses_api: Some(true),
                            output_version: Some("responses/v1".to_string()),
                            extra: serde_json::Map::new(),
                        },
                    },
                ],
            },
            ..Default::default()
        };

        let kernel_config = KernelConfig::from_unified_config(&config).unwrap();

        assert_eq!(kernel_config.llm.provider_type, ProviderType::OpenAI);
        assert_eq!(kernel_config.llm.model, "gpt-4o");
        assert_eq!(kernel_config.llm.api_key.as_deref(), Some("$OPENAI_API_KEY"));
        assert_eq!(kernel_config.llm.base_url.as_deref(), Some("https://api.openai.com/v1"));
        assert_eq!(kernel_config.llm.api_version.as_deref(), Some("responses/v1"));
        assert_eq!(kernel_config.llm.generation.max_tokens, Some(8192));
        assert_eq!(kernel_config.llm.generation.temperature, Some(0.4));
        assert_eq!(
            kernel_config.llm.extra.get("use_responses_api"),
            Some(&serde_json::Value::Bool(true))
        );
    }

    #[test]
    fn test_from_unified_config_falls_back_to_first_model_when_default_missing() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: String::new(),
                entries: vec![ModelEntry {
                    name: "claude-sonnet".to_string(),
                    display_name: "Claude Sonnet".to_string(),
                    provider: "anthropic".to_string(),
                    model_id: "claude-3-7-sonnet".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let kernel_config = KernelConfig::from_unified_config(&config).unwrap();

        assert_eq!(kernel_config.llm.provider_type, ProviderType::Anthropic);
        assert_eq!(kernel_config.llm.model, "claude-3-7-sonnet");
    }

    #[test]
    fn test_from_unified_config_surfaces_unknown_default_model_error() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: "missing".to_string(),
                entries: vec![ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "open_ai".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let error = KernelConfig::from_unified_config(&config).unwrap_err();

        assert!(error.to_string().contains("default_model 'missing'"));
    }

    #[test]
    fn test_from_unified_config_rejects_unsupported_provider_mapping() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: "custom".to_string(),
                entries: vec![ModelEntry {
                    name: "custom".to_string(),
                    display_name: "Custom".to_string(),
                    provider: "custom-provider".to_string(),
                    model_id: "custom-model".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let error = KernelConfig::from_unified_config(&config).unwrap_err();

        assert!(error.to_string().contains("unsupported unified model provider 'custom-provider'"));
    }

    #[test]
    fn test_resolve_runtime_uses_unified_mode_when_governance_matches_legacy_model() {
        let root = temp_test_dir();
        let governance_root = root.join("governance");
        fs::create_dir_all(&governance_root).unwrap();
        fs::write(
            governance_root.join("models.yaml"),
            r#"
default_model: gpt-4
"#,
        )
        .unwrap();
        fs::write(
            root.join("extensions_config.json"),
            r#"{
  "mcpServers": {
    "github": {
      "enabled": true,
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {"GITHUB_TOKEN": "$GITHUB_TOKEN"},
      "description": "GitHub"
    }
  },
  "skills": {}
}"#,
        )
        .unwrap();
        let config_path = root.join("config.yaml");
        fs::write(
            &config_path,
            format!(
                r#"
log_level: info
extensions_config_path: {}
llm:
  provider_type: open_ai
  model: gpt-4
  api_key: $OPENAI_API_KEY
migration:
  mode: unified
  governance_root: {}
"#,
                root.join("extensions_config.json").display(),
                governance_root.display(),
            ),
        )
        .unwrap();

        let resolved = KernelConfig::resolve_runtime(&config_path).unwrap();

        assert_eq!(resolved.resolution.requested_mode, KernelRuntimeMode::Unified);
        assert_eq!(resolved.resolution.effective_mode, KernelRuntimeMode::Unified);
        assert_eq!(resolved.config.llm.provider_type, ProviderType::OpenAI);
        assert_eq!(resolved.config.llm.model, "gpt-4");
        assert_eq!(resolved.config.mcp.servers.len(), 1);
        assert_eq!(resolved.config.mcp.servers[0].name, "github");
        assert_eq!(resolved.config.mcp.servers[0].transport, "stdio");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_resolve_runtime_auto_rolls_back_to_legacy_mode_on_unified_failure() {
        let root = temp_test_dir();
        let governance_root = root.join("governance");
        fs::create_dir_all(&governance_root).unwrap();
        fs::write(
            governance_root.join("models.yaml"),
            r#"
default_model: does-not-exist
"#,
        )
        .unwrap();
        let config_path = root.join("config.yaml");
        fs::write(
            &config_path,
            format!(
                r#"
llm:
  provider_type: anthropic
  model: claude-3-7-sonnet
migration:
  mode: auto
  governance_root: {}
  rollback_on_unified_failure: true
"#,
                governance_root.display(),
            ),
        )
        .unwrap();

        let resolved = KernelConfig::resolve_runtime(&config_path).unwrap();

        assert_eq!(resolved.resolution.requested_mode, KernelRuntimeMode::Auto);
        assert_eq!(resolved.resolution.effective_mode, KernelRuntimeMode::Legacy);
        assert!(resolved.resolution.rolled_back());
        assert!(resolved
            .resolution
            .rollback_reason
            .as_deref()
            .unwrap()
            .contains("default_model 'does-not-exist'"));
        assert_eq!(resolved.config.llm.provider_type, ProviderType::Anthropic);
        assert_eq!(resolved.config.llm.model, "claude-3-7-sonnet");

        fs::remove_dir_all(root).unwrap();
    }
}
