use crate::config::KernelConfig;
use crate::events::{EventBus, KernelStartedEvent, KernelStoppedEvent};
use crate::lifecycle::{shutdown_stages, LifecycleManager, LifecycleStage};
use crate::{KernelError, KernelResult};
use llm_providers::{LLMProvider, ProviderConfig, ProviderType};
use mcp_bridge::McpBridgeManager;
use plugin_system::PluginManager;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Open Harness Agent Kernel
#[derive(Clone)]
pub struct AgentKernel {
    config: KernelConfig,
    event_bus: Arc<EventBus>,
    lifecycle_manager: Arc<LifecycleManager>,
    plugin_manager: Arc<RwLock<Option<PluginManager>>>,
    llm_provider: Arc<RwLock<Option<Box<dyn LLMProvider>>>>,
    mcp_bridge: Arc<RwLock<Option<McpBridgeManager>>>,
    state: Arc<RwLock<KernelState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernelState {
    Created,
    Initialized,
    Running,
    Stopped,
}

impl AgentKernel {
    /// 创建新的 kernel 实例
    pub fn new(config: KernelConfig) -> Self {
        Self {
            config,
            event_bus: Arc::new(EventBus::new()),
            lifecycle_manager: Arc::new(LifecycleManager::new()),
            plugin_manager: Arc::new(RwLock::new(None)),
            llm_provider: Arc::new(RwLock::new(None)),
            mcp_bridge: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(KernelState::Created)),
        }
    }

    /// 获取 LLM provider（如果已初始化）
    pub fn llm_provider(&self) -> Option<Arc<RwLock<Option<Box<dyn LLMProvider>>>>> {
        Some(self.llm_provider.clone())
    }

    /// 获取 MCP bridge（如果已初始化）
    pub fn mcp_bridge(&self) -> Option<Arc<RwLock<Option<McpBridgeManager>>>> {
        Some(self.mcp_bridge.clone())
    }

    /// 获取事件总线
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    /// 获取生命周期管理器
    pub fn lifecycle_manager(&self) -> &Arc<LifecycleManager> {
        &self.lifecycle_manager
    }

    /// 获取配置
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// 初始化 kernel
    pub async fn initialize(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;
        if *state != KernelState::Created {
            return Err(KernelError::Lifecycle("Kernel already initialized".to_string()));
        }

        tracing::info!("Initializing Open Harness Agent Kernel...");

        // 初始化默认 LLM provider（rig 作为默认）
        let provider_config = ProviderConfig::new(ProviderType::Rig, "gpt-4");
        let provider = llm_providers::rig::RigProvider::new(&provider_config).map_err(|e| {
            KernelError::Initialization(format!("Failed to create LLM provider: {}", e))
        })?;
        *self.llm_provider.write().await = Some(Box::new(provider));

        // 初始化 MCP bridge
        let mcp_config = mcp_bridge::McpBridgeConfig::default();
        let mcp_bridge = mcp_bridge::McpBridgeManager::new(mcp_config);
        mcp_bridge.initialize().await.map_err(|e| {
            KernelError::Initialization(format!("Failed to initialize MCP bridge: {}", e))
        })?;
        *self.mcp_bridge.write().await = Some(mcp_bridge);

        // 初始化插件管理器
        let plugin_manager = PluginManager::new(self.config.plugins_dir.clone());
        plugin_manager.discover_plugins().await?;
        *self.plugin_manager.write().await = Some(plugin_manager);

        // 运行初始化生命周期
        self.lifecycle_manager
            .run_stages(&[
                LifecycleStage::BeforeInit,
                LifecycleStage::Init,
                LifecycleStage::AfterInit,
            ])
            .await?;

        *state = KernelState::Initialized;
        tracing::info!("Kernel initialized successfully");

        Ok(())
    }

    /// 启动 kernel
    pub async fn start(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;
        if *state != KernelState::Initialized {
            return Err(KernelError::Lifecycle(format!(
                "Cannot start kernel in state: {:?}",
                state
            )));
        }

        tracing::info!("Starting Open Harness Agent Kernel...");

        // 运行启动生命周期
        self.lifecycle_manager
            .run_stages(&[
                LifecycleStage::BeforeStart,
                LifecycleStage::Start,
                LifecycleStage::AfterStart,
            ])
            .await?;

        // 加载并启动插件
        if let Some(plugin_manager) = &*self.plugin_manager.read().await {
            plugin_manager.load_all_plugins().await?;
        }

        // 发布启动事件
        self.event_bus.publish(KernelStartedEvent).await?;

        *state = KernelState::Running;
        tracing::info!("Kernel started successfully");

        Ok(())
    }

    /// 停止 kernel
    pub async fn stop(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;
        if *state != KernelState::Running {
            return Err(KernelError::Lifecycle(format!(
                "Cannot stop kernel in state: {:?}",
                state
            )));
        }

        tracing::info!("Stopping Open Harness Agent Kernel...");

        // 发布停止事件
        self.event_bus.publish(KernelStoppedEvent).await?;

        // 停止插件
        if let Some(plugin_manager) = &*self.plugin_manager.read().await {
            plugin_manager.unload_all_plugins().await?;
        }

        // 运行停止生命周期
        self.lifecycle_manager.run_stages(&shutdown_stages()).await?;

        *state = KernelState::Stopped;
        tracing::info!("Kernel stopped successfully");

        Ok(())
    }

    /// 便捷方法：初始化并启动
    pub async fn initialize_and_start(&self) -> KernelResult<()> {
        self.initialize().await?;
        self.start().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kernel_lifecycle() {
        let config = KernelConfig::default();
        let kernel = AgentKernel::new(config);

        assert!(*kernel.state.read().await == KernelState::Created);

        kernel.initialize().await.unwrap();
        assert!(*kernel.state.read().await == KernelState::Initialized);

        kernel.start().await.unwrap();
        assert!(*kernel.state.read().await == KernelState::Running);

        kernel.stop().await.unwrap();
        assert!(*kernel.state.read().await == KernelState::Stopped);
    }
}
