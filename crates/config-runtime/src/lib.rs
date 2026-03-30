//! Aggregated configuration for open-harness services.

use figment::{
    providers::{Env, Format, Serialized, Yaml},
    Figment,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::RwLock, time::SystemTime};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("figment: {0}")]
    Figment(String),
}

/// Configuration for local filesystem backend.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LocalFsConfig {
    #[serde(default = "default_local_fs_root")]
    pub root: String,
}

/// Configuration for SQLite backend.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SqliteConfig {
    #[serde(default)]
    pub url: Option<String>,
}

/// Configuration for PostgreSQL backend.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PostgresConfig {
    #[serde(default)]
    pub url: Option<String>,
}

/// Configuration for Redis backend.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RedisConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Configuration for S3-compatible object storage backend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct S3Config {
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default = "default_s3_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl Default for S3Config {
    fn default() -> Self {
        Self { bucket: None, prefix: default_s3_prefix(), region: None, endpoint: None }
    }
}

/// Unified storage configuration with nested backend-specific settings.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StorageConfig {
    #[serde(default = "default_storage_mode")]
    pub mode: String,

    #[serde(default)]
    pub local_fs: LocalFsConfig,

    #[serde(default)]
    pub sqlite: SqliteConfig,

    #[serde(default)]
    pub postgres: PostgresConfig,

    #[serde(default)]
    pub redis: RedisConfig,

    #[serde(default)]
    pub s3: S3Config,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub bind: String,
    pub langgraph_upstream: String,
    #[serde(default = "default_auth")]
    pub auth: AuthConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManageConfig {
    pub bind: String,
    #[serde(default = "default_langgraph")]
    pub langgraph_url: String,
    /// **Deprecated for durable I/O**: not used by manage/orchestrator for uploads, artifacts, or
    /// thread lifecycle; those go through `storage.mode` + `StorageRegistry`. Kept for config-file
    /// compatibility and external tooling that still references the path.
    #[serde(default = "default_threads_root")]
    pub threads_root: String,
    #[serde(default)]
    pub sqlite_url: Option<String>,
    #[serde(default)]
    pub postgres_url: Option<String>,
    #[serde(default = "default_auth")]
    pub auth: AuthConfig,
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChannelsConfig {
    #[serde(default = "default_channel_enabled")]
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_engine")]
    pub engine: String,
    /// Directory containing governance YAML (`models.yaml`, `tools.yaml`, `policies.yaml`, `subagents.yaml`).
    #[serde(default = "default_governance_root")]
    pub governance_root: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_keys: Vec<String>,
    #[serde(default)]
    pub bearer_tokens: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    pub name: String,
    pub display_name: String,
    #[serde(rename = "use")]
    pub use_provider: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub use_responses_api: Option<bool>,
    #[serde(default)]
    pub output_version: Option<String>,
}

fn default_langgraph() -> String {
    "http://127.0.0.1:2024".into()
}

fn default_threads_root() -> String {
    ".deer-flow/threads".into()
}

fn default_storage_mode() -> String {
    "local_fs".into()
}

fn default_local_fs_root() -> String {
    ".deer-flow/local-fs".into()
}

fn default_s3_prefix() -> String {
    "open-harness".into()
}

fn default_auth() -> AuthConfig {
    AuthConfig { enabled: false, api_keys: Vec::new(), bearer_tokens: Vec::new() }
}

fn default_config_path() -> String {
    "config.yaml".into()
}

fn default_models() -> Vec<ModelConfig> {
    vec![ModelConfig {
        name: "gpt-4".into(),
        display_name: "GPT-4".into(),
        use_provider: "langchain_openai:ChatOpenAI".into(),
        model: "gpt-4".into(),
        api_key: Some("$OPENAI_API_KEY".into()),
        max_tokens: Some(4096),
        temperature: Some(0.7),
        base_url: None,
        use_responses_api: None,
        output_version: None,
    }]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_config_path")]
    pub config_path: String,
    #[serde(default = "default_storage")]
    pub storage: StorageConfig,
    #[serde(default = "default_models")]
    pub models: Vec<ModelConfig>,
    #[serde(default = "default_gateway")]
    pub gateway: GatewayConfig,
    #[serde(default = "default_manage")]
    pub manage: ManageConfig,
    #[serde(default = "default_channels")]
    pub channels: ChannelsConfig,
    #[serde(default = "default_runtime")]
    pub runtime: RuntimeConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_path: default_config_path(),
            storage: default_storage(),
            models: default_models(),
            gateway: default_gateway(),
            manage: default_manage(),
            channels: default_channels(),
            runtime: default_runtime(),
        }
    }
}

fn default_gateway() -> GatewayConfig {
    GatewayConfig {
        bind: "0.0.0.0:8080".into(),
        langgraph_upstream: "http://127.0.0.1:2024".into(),
        auth: default_auth(),
    }
}

fn default_manage() -> ManageConfig {
    ManageConfig {
        bind: "0.0.0.0:8081".into(),
        langgraph_url: default_langgraph(),
        threads_root: default_threads_root(),
        sqlite_url: None,
        postgres_url: None,
        auth: default_auth(),
        webhook_secret: None,
    }
}

fn default_channel_enabled() -> Vec<String> {
    vec!["dingtalk".into(), "wecom".into()]
}

fn default_channels() -> ChannelsConfig {
    ChannelsConfig { enabled: default_channel_enabled() }
}

fn default_runtime_engine() -> String {
    "inner".to_string()
}

fn default_governance_root() -> String {
    "governance".into()
}

fn default_runtime() -> RuntimeConfig {
    RuntimeConfig { engine: default_runtime_engine(), governance_root: default_governance_root() }
}

fn default_storage() -> StorageConfig {
    StorageConfig {
        mode: default_storage_mode(),
        local_fs: LocalFsConfig { root: default_local_fs_root() },
        sqlite: SqliteConfig::default(),
        postgres: PostgresConfig::default(),
        redis: RedisConfig::default(),
        s3: S3Config::default(),
    }
}

#[derive(Debug, Clone)]
struct ConfigSnapshot {
    config: AppConfig,
    config_path: PathBuf,
    modified_at: Option<SystemTime>,
}

static SNAPSHOT: Lazy<RwLock<Option<ConfigSnapshot>>> = Lazy::new(|| RwLock::new(None));

fn app_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("OPEN_HARNESS_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    PathBuf::from(default_config_path())
}

fn ensure_default_config(path: &PathBuf) {
    if path.exists() {
        return;
    }
    let default_yaml = r#"storage:
  mode: local_fs
  local_fs_root: .deer-flow/local-fs
  sqlite_url: null
  postgres_url: null
  redis_url: null
  s3_bucket: null
  s3_prefix: open-harness

models:
  - name: gpt-4
    display_name: GPT-4
    use_provider: langchain_openai:ChatOpenAI
    model: gpt-4
    api_key: $OPENAI_API_KEY
    max_tokens: 4096
    temperature: 0.7
  - name: openrouter-gemini-2.5-flash
    display_name: Gemini 2.5 Flash (OpenRouter)
    use_provider: langchain_openai:ChatOpenAI
    model: google/gemini-2.5-flash-preview
    api_key: $OPENAI_API_KEY
    base_url: https://openrouter.ai/api/v1
  - name: gpt-5-responses
    display_name: GPT-5 (Responses API)
    use_provider: langchain_openai:ChatOpenAI
    model: gpt-5
    api_key: $OPENAI_API_KEY
    use_responses_api: true
    output_version: responses/v1

gateway:
  bind: 0.0.0.0:8080
  langgraph_upstream: http://127.0.0.1:2024
  auth:
    enabled: false
    api_keys: []
    bearer_tokens: []

manage:
  bind: 0.0.0.0:8081
  langgraph_url: http://127.0.0.1:2024
  threads_root: .deer-flow/threads
  auth:
    enabled: false
    api_keys: []
    bearer_tokens: []
  webhook_secret: null

channels:
  enabled:
    - dingtalk
    - wecom

runtime:
  engine: inner
  governance_root: governance
"#;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, default_yaml);
}

/// Load config from `config.yaml` then `OPEN_HARNESS_*` env overrides.
pub fn load_from_env() -> Result<AppConfig, ConfigError> {
    let cfg_path = app_config_path();
    ensure_default_config(&cfg_path);
    Figment::from(Serialized::defaults(AppConfig::default()))
        .merge(Yaml::file(&cfg_path))
        .merge(Env::prefixed("OPEN_HARNESS_").split("__"))
        .extract()
        .map_err(|e| ConfigError::Figment(e.to_string()))
}

/// Merge env over defaults (never fails).
pub fn load_or_default() -> AppConfig {
    match load_from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::error!(
                config_path = %app_config_path().display(),
                error = %err,
                "failed to load config, falling back to defaults"
            );
            AppConfig::default()
        }
    }
}

fn read_modified_at(path: &PathBuf) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

/// Load config with process-local cache and mtime-based reload.
pub fn load_cached_or_default() -> AppConfig {
    let cfg_path = app_config_path();
    let modified_at = read_modified_at(&cfg_path);

    if let Some(snapshot) = SNAPSHOT.read().ok().and_then(|g| g.clone()) {
        if snapshot.config_path == cfg_path && snapshot.modified_at == modified_at {
            return snapshot.config;
        }
    }

    let loaded = load_or_default();
    if let Ok(mut guard) = SNAPSHOT.write() {
        *guard =
            Some(ConfigSnapshot { config: loaded.clone(), config_path: cfg_path, modified_at });
    }
    loaded
}

/// Force reload from disk/env and refresh cache.
pub fn reload_cached() -> Result<AppConfig, ConfigError> {
    let cfg_path = app_config_path();
    let loaded = load_from_env()?;
    let modified_at = read_modified_at(&cfg_path);
    if let Ok(mut guard) = SNAPSHOT.write() {
        *guard =
            Some(ConfigSnapshot { config: loaded.clone(), config_path: cfg_path, modified_at });
    }
    Ok(loaded)
}

pub fn resolve_env_var_ref(value: &str) -> String {
    if let Some(stripped) = value.strip_prefix('$') {
        return std::env::var(stripped).unwrap_or_default();
    }
    value.to_string()
}

pub fn set_test_config(c: AppConfig) {
    *SNAPSHOT.write().expect("snapshot write") =
        Some(ConfigSnapshot { config: c, config_path: app_config_path(), modified_at: None });
}

/// Load unified configuration from AppConfig and governance directory.
pub fn load_unified_config(
    app_cfg: &AppConfig,
) -> Result<unified_config::UnifiedConfig, unified_config::loader::ConfigLoaderError> {
    // Convert AppConfig to AppConfigRef
    let app_cfg_ref = unified_config::loader::AppConfigRef {
        models: app_cfg
            .models
            .iter()
            .map(|m| unified_config::loader::ModelConfigRef {
                name: m.name.clone(),
                display_name: m.display_name.clone(),
                use_provider: m.use_provider.clone(),
                model: m.model.clone(),
                api_key: m.api_key.clone(),
                max_tokens: m.max_tokens,
                temperature: m.temperature,
                base_url: m.base_url.clone(),
                use_responses_api: m.use_responses_api,
                output_version: m.output_version.clone(),
            })
            .collect(),
    };

    unified_config::loader::load_unified_config(&app_cfg_ref, &app_cfg.runtime.governance_root)
}

/// Create a ConfigManager with hot-reload support from AppConfig.
pub fn create_config_manager(
    app_cfg: &AppConfig,
) -> Result<unified_config::ConfigManager, unified_config::loader::ConfigLoaderError> {
    let config = load_unified_config(app_cfg)?;
    Ok(unified_config::ConfigManager::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_loads() {
        let c = load_or_default();
        assert!(c.gateway.bind.contains("8080"));
        assert!(c.manage.bind.contains("8081"));
    }
}
