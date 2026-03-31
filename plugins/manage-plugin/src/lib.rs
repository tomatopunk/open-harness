//! Manage Plugin - Management API
//!
//! Provides management API for agents, models, and configurations.

mod api;
mod error;

use async_trait::async_trait;
use plugin_system::{BasePlugin, Plugin, PluginContext, PluginManifest, PluginResult, PluginState};
use tokio::sync::RwLock;

pub use error::ManagePluginError;

/// Manage plugin configuration
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ManagePluginConfig {
    /// Bind address
    #[serde(default = "default_bind")]
    pub bind: String,
}

impl Default for ManagePluginConfig {
    fn default() -> Self {
        Self { bind: default_bind() }
    }
}

fn default_bind() -> String {
    "0.0.0.0:8081".to_string()
}

/// Manage Plugin
pub struct ManagePlugin {
    base: BasePlugin,
    server_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    config: ManagePluginConfig,
}

impl ManagePlugin {
    /// Create a new Manage plugin
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            base: BasePlugin::new(manifest),
            server_handle: RwLock::new(None),
            config: ManagePluginConfig::default(),
        }
    }
}

#[async_trait]
impl Plugin for ManagePlugin {
    fn manifest(&self) -> &PluginManifest {
        self.base.manifest()
    }

    fn state(&self) -> PluginState {
        self.base.state()
    }

    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Loading Manage plugin...");

        // Load config from context
        if let Some(config) = &ctx.config {
            if let Ok(parsed) = serde_json::from_value::<ManagePluginConfig>(config.clone()) {
                self.config = parsed;
            }
        }

        self.base.load(ctx).await
    }

    async fn initialize(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Initializing Manage plugin...");
        self.base.initialize(ctx).await
    }

    async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Starting Manage plugin on {}...", self.config.bind);

        // Start HTTP server
        let app = api::create_router();
        let bind = self.config.bind.clone();

        let handle = tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("Manage server error: {}", e);
            }
        });

        *self.server_handle.write().await = Some(handle);
        self.base.start(ctx).await
    }

    async fn stop(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Stopping Manage plugin...");

        if let Some(handle) = self.server_handle.write().await.take() {
            handle.abort();
        }

        self.base.stop(ctx).await
    }

    async fn unload(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        tracing::info!("Unloading Manage plugin...");
        self.base.unload(ctx).await
    }
}
