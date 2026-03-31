use crate::{PluginManifest, PluginResult};
use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已发现但未加载
    Discovered,
    /// 已加载但未初始化
    Loaded,
    /// 已初始化
    Initialized,
    /// 正在运行
    Running,
    /// 已停止
    Stopped,
    /// 已卸载
    Unloaded,
}

/// 插件上下文
pub struct PluginContext {
    /// 工作目录
    pub workspace_dir: std::path::PathBuf,

    /// 插件配置
    pub config: Option<Value>,

    /// 共享状态
    pub shared_state: Arc<parking_lot::RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>>,
}

impl PluginContext {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self {
            workspace_dir,
            config: None,
            shared_state: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// 获取共享状态
    pub fn get_shared<T: 'static + Clone>(&self, key: &str) -> Option<T> {
        let state = self.shared_state.read();
        state.get(key).and_then(|v| v.downcast_ref::<T>()).cloned()
    }

    /// 设置共享状态
    pub fn set_shared<T: 'static + Send + Sync>(&self, key: String, value: T) {
        let mut state = self.shared_state.write();
        state.insert(key, Box::new(value));
    }
}

/// 插件 trait
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// 获取插件清单
    fn manifest(&self) -> &PluginManifest;

    /// 获取插件状态
    fn state(&self) -> PluginState;

    /// 加载插件
    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()>;

    /// 初始化插件
    async fn initialize(&mut self, ctx: &PluginContext) -> PluginResult<()>;

    /// 启动插件
    async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()>;

    /// 停止插件
    async fn stop(&mut self, ctx: &PluginContext) -> PluginResult<()>;

    /// 卸载插件
    async fn unload(&mut self, ctx: &PluginContext) -> PluginResult<()>;
}

/// 基础插件实现（可作为其他插件的基类）
pub struct BasePlugin {
    manifest: PluginManifest,
    state: PluginState,
}

impl BasePlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self { manifest, state: PluginState::Discovered }
    }

    #[allow(dead_code)]
    pub fn set_state(&mut self, state: PluginState) {
        self.state = state;
    }
}

#[async_trait]
impl Plugin for BasePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn state(&self) -> PluginState {
        self.state
    }

    async fn load(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Loaded;
        Ok(())
    }

    async fn initialize(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Initialized;
        Ok(())
    }

    async fn start(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Running;
        Ok(())
    }

    async fn stop(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Stopped;
        Ok(())
    }

    async fn unload(&mut self, _ctx: &PluginContext) -> PluginResult<()> {
        self.state = PluginState::Unloaded;
        Ok(())
    }
}
