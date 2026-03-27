//! Aggregated configuration for open-harness services.

use figment::{
    providers::{Env, Serialized},
    Figment,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("figment: {0}")]
    Figment(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    pub bind: String,
    pub langgraph_upstream: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManageConfig {
    pub bind: String,
    pub storage_backend: String,
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

fn default_langgraph() -> String {
    "http://127.0.0.1:2024".into()
}

fn default_threads_root() -> String {
    ".deer-flow/threads".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_gateway")]
    pub gateway: GatewayConfig,
    #[serde(default = "default_manage")]
    pub manage: ManageConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { gateway: default_gateway(), manage: default_manage() }
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
        storage_backend: "sqlite".into(),
        langgraph_url: default_langgraph(),
        threads_root: default_threads_root(),
        sqlite_url: None,
        postgres_url: None,
    }
}

static SNAPSHOT: Lazy<RwLock<Option<AppConfig>>> = Lazy::new(|| RwLock::new(None));

/// Load config from `OPEN_HARNESS_*` env (nested with `__`), falling back to defaults.
pub fn load_from_env() -> Result<AppConfig, ConfigError> {
    Figment::from(Serialized::defaults(AppConfig::default()))
        .merge(Env::prefixed("OPEN_HARNESS_").split("__"))
        .extract()
        .map_err(|e| ConfigError::Figment(e.to_string()))
}

/// Merge env over defaults (never fails).
pub fn load_or_default() -> AppConfig {
    load_from_env().unwrap_or_default()
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
