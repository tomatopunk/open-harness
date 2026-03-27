//! Aggregated configuration for open-harness services.

use figment::{
    providers::{Env, Format, Serialized, Yaml},
    Figment,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::RwLock};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("figment: {0}")]
    Figment(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_mode")]
    pub mode: String,
    #[serde(default = "default_local_fs_root")]
    pub local_fs_root: String,
    #[serde(default)]
    pub sqlite_url: Option<String>,
    #[serde(default)]
    pub postgres_url: Option<String>,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub s3_bucket: Option<String>,
    #[serde(default = "default_s3_prefix")]
    pub s3_prefix: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub bind: String,
    pub langgraph_upstream: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManageConfig {
    pub bind: String,
    #[serde(default = "default_langgraph")]
    pub langgraph_url: String,
    /// Local thread workspace root (e.g. `.deer-flow/threads`).
    #[serde(default = "default_threads_root")]
    pub threads_root: String,
    #[serde(default)]
    pub sqlite_url: Option<String>,
    #[serde(default)]
    pub postgres_url: Option<String>,
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            config_path: default_config_path(),
            storage: default_storage(),
            models: default_models(),
            gateway: default_gateway(),
            manage: default_manage(),
        }
    }
}

fn default_gateway() -> GatewayConfig {
    GatewayConfig {
        bind: "0.0.0.0:8080".into(),
        langgraph_upstream: "http://127.0.0.1:2024".into(),
    }
}

fn default_manage() -> ManageConfig {
    ManageConfig {
        bind: "0.0.0.0:8081".into(),
        langgraph_url: default_langgraph(),
        threads_root: default_threads_root(),
        sqlite_url: None,
        postgres_url: None,
    }
}

fn default_storage() -> StorageConfig {
    StorageConfig {
        mode: default_storage_mode(),
        local_fs_root: default_local_fs_root(),
        sqlite_url: None,
        postgres_url: None,
        redis_url: None,
        s3_bucket: None,
        s3_prefix: default_s3_prefix(),
    }
}

static SNAPSHOT: Lazy<RwLock<Option<AppConfig>>> = Lazy::new(|| RwLock::new(None));

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
    load_from_env().unwrap_or_default()
}

pub fn resolve_env_var_ref(value: &str) -> String {
    if let Some(stripped) = value.strip_prefix('$') {
        return std::env::var(stripped).unwrap_or_default();
    }
    value.to_string()
}

pub fn set_test_config(c: AppConfig) {
    *SNAPSHOT.write().unwrap() = Some(c);
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
