//! Gateway Plugin - OpenAI Compatible API
//!
//! 提供 OpenAI 兼容的聊天完成 API

mod api;
mod error;

use async_trait::async_trait;
use plugin_system::{
    BasePlugin, Plugin, PluginContext, PluginError, PluginManifest, PluginResult, PluginState,
};
use tokio::sync::RwLock;

pub use error::GatewayPluginError;

/// Gateway 插件
pub struct GatewayPlugin {
    base: BasePlugin,
    server_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    config: GatewayPluginConfig,
}

/// Gateway 插件配置
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GatewayPluginConfig {
    /// 绑定地址
    #[serde(default = "default_bind")]
    pub bind: String,

    /// 启用 CORS
    #[serde(default = "default_true")]
    pub cors: bool,
}

impl Default for GatewayPluginConfig {
    fn default() -> Self {
        Self { bind: default_bind(), cors: true }
    }
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_true() -> bool {
    true
}

impl GatewayPlugin {
    /// 创建新的 Gateway 插件
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            base: BasePlugin::new(manifest),
            server_handle: RwLock::new(None),
            config: GatewayPluginConfig::default(),
        }
    }
}

#[async_trait]
impl Plugin for GatewayPlugin {
    fn manifest(&self) -> &PluginManifest {
        self.base.manifest()
    }

    fn state(&self) -> PluginState {
        self.base.state()
    }

    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Loading Gateway plugin...");

        if let Some(config) = &ctx.config {
            self.config =
                serde_json::from_value::<GatewayPluginConfig>(config.clone()).map_err(|error| {
                    PluginError::InvalidConfiguration {
                        plugin: self.manifest().name.clone(),
                        details: error.to_string(),
                    }
                })?;
        }

        self.base.load(ctx).await
    }

    async fn initialize(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Initializing Gateway plugin...");
        self.base.initialize(ctx).await
    }

    async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Starting Gateway plugin on {}...", self.config.bind);

        let app = api::create_router();
        let listener = tokio::net::TcpListener::bind(&self.config.bind).await.map_err(|error| {
            PluginError::InitializationFailed(format!(
                "failed to bind gateway listener on {}: {}",
                self.config.bind, error
            ))
        })?;

        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("Gateway server error: {}", e);
            }
        });

        *self.server_handle.write().await = Some(handle);
        self.base.start(ctx).await
    }

    async fn stop(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Stopping Gateway plugin...");

        if let Some(handle) = self.server_handle.write().await.take() {
            handle.abort();
        }

        self.base.stop(ctx).await
    }

    async fn unload(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Unloading Gateway plugin...");
        self.base.unload(ctx).await
    }
}
