//! DingTalk channel plugin for Open Harness

use async_trait::async_trait;
use plugin_system::{BasePlugin, Plugin, PluginContext, PluginManifest, PluginResult, PluginState};

/// DingTalk plugin
pub struct DingTalkPlugin {
    base: BasePlugin,
}

impl DingTalkPlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self { base: BasePlugin::new(manifest) }
    }
}

#[async_trait]
impl Plugin for DingTalkPlugin {
    fn manifest(&self) -> &PluginManifest {
        self.base.manifest()
    }

    fn state(&self) -> PluginState {
        self.base.state()
    }

    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.base.load(ctx).await
    }

    async fn initialize(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.base.initialize(ctx).await
    }

    async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.base.start(ctx).await
    }

    async fn stop(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.base.stop(ctx).await
    }

    async fn unload(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        self.base.unload(ctx).await
    }
}
