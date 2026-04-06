use crate::agent_loop::AgentLoop;
use crate::channel_manager::ChannelManager;
use crate::config::KernelConfig;
use crate::events::{EventBus, KernelStartedEvent, KernelStoppedEvent};
use crate::hooks::HookSystem;
use crate::lifecycle::{shutdown_stages, LifecycleManager, LifecycleStage};
use crate::{KernelError, KernelResult};
use llm_providers::{create_provider, LLMProvider};
use mcp_bridge::McpBridgeManager;
use plugin_system::PluginManager;
use state_abstraction::memory_system::{MemorySystem, MemorySystemConfig};
use state_abstraction::traits::MemoryStore;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::sync::RwLock;

/// Open Harness Agent Kernel
#[derive(Clone)]
pub struct AgentKernel {
    config: KernelConfig,
    event_bus: Arc<EventBus>,
    lifecycle_manager: Arc<LifecycleManager>,
    plugin_manager: Arc<OnceCell<PluginManager>>,
    llm_provider: Arc<OnceCell<Box<dyn LLMProvider>>>,
    mcp_bridge: Arc<OnceCell<McpBridgeManager>>,
    hooks: Arc<HookSystem>,
    agent_loop: Arc<OnceCell<AgentLoop>>,
    memory_system: Arc<OnceCell<MemorySystem<Box<dyn MemoryStore>>>>,
    channel_manager: Arc<OnceCell<ChannelManager>>,
    state: Arc<RwLock<KernelState>>,
}

/// Kernel lifecycle state
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
            plugin_manager: Arc::new(OnceCell::new()),
            llm_provider: Arc::new(OnceCell::new()),
            mcp_bridge: Arc::new(OnceCell::new()),
            hooks: Arc::new(HookSystem::new()),
            agent_loop: Arc::new(OnceCell::new()),
            memory_system: Arc::new(OnceCell::new()),
            channel_manager: Arc::new(OnceCell::new()),
            state: Arc::new(RwLock::new(KernelState::Created)),
        }
    }

    /// 获取 LLM provider（如果已初始化）
    pub fn llm_provider(&self) -> Option<&dyn LLMProvider> {
        self.llm_provider.get().map(|b| b.as_ref())
    }

    /// 获取 MCP bridge（如果已初始化）
    pub fn mcp_bridge(&self) -> Option<&McpBridgeManager> {
        self.mcp_bridge.get()
    }

    /// 获取事件总线
    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    /// 获取钩子系统
    pub fn hooks(&self) -> &Arc<HookSystem> {
        &self.hooks
    }

    /// 获取 Agent Loop（如果已初始化）
    pub fn agent_loop(&self) -> Option<&AgentLoop> {
        self.agent_loop.get()
    }

    /// 获取记忆系统
    pub fn memory_system(&self) -> Option<&MemorySystem<Box<dyn MemoryStore>>> {
        self.memory_system.get()
    }

    /// 获取渠道管理器
    pub fn channel_manager(&self) -> Option<&ChannelManager> {
        self.channel_manager.get()
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

        self.initialize_provider_adapter()?;
        self.initialize_mcp_adapter().await?;
        self.initialize_plugin_adapter().await?;
        self.initialize_loop_state_adapter()?;
        self.initialize_channel_adapter()?;
        self.run_initialize_lifecycle().await?;

        *state = KernelState::Initialized;
        tracing::info!("Kernel initialized successfully");

        Ok(())
    }

    fn initialize_provider_adapter(&self) -> KernelResult<()> {
        let provider = create_provider(&self.config.llm).map_err(|e| {
            KernelError::Initialization(format!("Failed to create LLM provider: {}", e))
        })?;
        self.llm_provider
            .set(provider)
            .map_err(|_| KernelError::Lifecycle("LLM provider already initialized".to_string()))
    }

    async fn initialize_mcp_adapter(&self) -> KernelResult<()> {
        let mcp_bridge = self.build_mcp_bridge().await?;
        self.mcp_bridge
            .set(mcp_bridge)
            .map_err(|_| KernelError::Lifecycle("MCP bridge already initialized".to_string()))
    }

    async fn build_mcp_bridge(&self) -> KernelResult<McpBridgeManager> {
        let mcp_bridge = mcp_bridge::McpBridgeManager::new(self.config.mcp.clone());
        mcp_bridge
            .initialize()
            .await
            .map_err(|error| KernelError::context("Failed to initialize MCP bridge", error))?;

        Ok(mcp_bridge)
    }

    async fn initialize_plugin_adapter(&self) -> KernelResult<()> {
        let plugin_manager = self.build_plugin_manager().await?;
        self.plugin_manager
            .set(plugin_manager)
            .map_err(|_| KernelError::Lifecycle("Plugin manager already initialized".to_string()))
    }

    async fn build_plugin_manager(&self) -> KernelResult<PluginManager> {
        let plugin_manager = PluginManager::new_with_workspace_dir(
            self.config.plugins_dir.clone(),
            self.config.workspace_root.clone(),
        );
        plugin_manager
            .discover_plugins()
            .await
            .map_err(|error| KernelError::context("Failed to discover plugins", error))?;
        Ok(plugin_manager)
    }

    fn initialize_loop_state_adapter(&self) -> KernelResult<()> {
        self.initialize_agent_loop_adapter()?;
        self.initialize_memory_adapter()?;
        Ok(())
    }

    fn initialize_agent_loop_adapter(&self) -> KernelResult<()> {
        if !self.config.agent_loop.enabled {
            return Ok(());
        }

        let agent_loop = AgentLoop::new(
            self.config.agent_loop.clone(),
            self.hooks.clone(),
            self.event_bus.clone(),
            self.llm_provider.clone(),
        );
        self.agent_loop
            .set(agent_loop)
            .map_err(|_| KernelError::Lifecycle("Agent Loop already initialized".to_string()))
    }

    fn initialize_memory_adapter(&self) -> KernelResult<()> {
        if !self.config.memory.enabled {
            return Ok(());
        }

        let memory_system = self.build_memory_system()?;
        self.memory_system
            .set(memory_system)
            .map_err(|_| KernelError::Lifecycle("Memory system already initialized".to_string()))
    }

    fn build_memory_system(&self) -> KernelResult<MemorySystem<Box<dyn MemoryStore>>> {
        let memory_config = MemorySystemConfig {
            max_facts: self.config.memory.max_facts,
            fact_confidence_threshold: self.config.memory.fact_confidence_threshold,
            max_injection_tokens: self.config.memory.max_injection_tokens,
        };
        let storage_config = self.build_state_storage_config();
        let store = state_abstraction::create_memory_store(
            &storage_config,
            &self.config.workspace_root,
            &self.config.memory.storage_path,
        )
        .map_err(|error| KernelError::context("Failed to create memory store", error))?;

        Ok(MemorySystem::new(memory_config, store))
    }

    fn build_state_storage_config(&self) -> state_abstraction::StorageConfig {
        state_abstraction::StorageConfig {
            mode: match self.config.storage.mode {
                crate::config::StorageMode::LocalFs => state_abstraction::StorageMode::LocalFs,
                crate::config::StorageMode::Sqlite => state_abstraction::StorageMode::Sqlite,
                crate::config::StorageMode::Postgres => state_abstraction::StorageMode::Postgres,
            },
            local_fs: self
                .config
                .storage
                .local_fs
                .as_ref()
                .map(|local_fs| state_abstraction::LocalFsConfig { root: local_fs.root.clone() }),
            sqlite: None,
        }
    }

    fn initialize_channel_adapter(&self) -> KernelResult<()> {
        let channel_manager = ChannelManager::new(self.config.channels.clone());
        self.channel_manager
            .set(channel_manager)
            .map_err(|_| KernelError::Lifecycle("Channel manager already initialized".to_string()))
    }

    async fn run_initialize_lifecycle(&self) -> KernelResult<()> {
        self.lifecycle_manager
            .run_stages(&[
                LifecycleStage::BeforeInit,
                LifecycleStage::InitConfig,
                LifecycleStage::InitProvider,
                LifecycleStage::InitMcp,
                LifecycleStage::InitPlugin,
                LifecycleStage::InitLoopState,
                LifecycleStage::Init,
                LifecycleStage::AfterInit,
            ])
            .await
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
        if let Some(plugin_manager) = self.plugin_manager.get() {
            plugin_manager
                .load_all_plugins()
                .await
                .map_err(|error| KernelError::context("Failed to load plugins", error))?;
            plugin_manager
                .initialize_all_plugins()
                .await
                .map_err(|error| KernelError::context("Failed to initialize plugins", error))?;
            plugin_manager
                .start_all_plugins()
                .await
                .map_err(|error| KernelError::context("Failed to start plugins", error))?;
        }

        // 启动所有渠道
        if let Some(channel_manager) = self.channel_manager.get() {
            channel_manager.start_all().await.map_err(|e| {
                KernelError::ExternalConnection(format!("Failed to start channels: {}", e))
            })?;
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

        // 停止所有渠道
        if let Some(channel_manager) = self.channel_manager.get() {
            channel_manager
                .stop_all()
                .await
                .map_err(|e| KernelError::Lifecycle(format!("Failed to stop channels: {}", e)))?;
        }

        // 停止插件
        if let Some(plugin_manager) = self.plugin_manager.get() {
            plugin_manager
                .stop_all_plugins()
                .await
                .map_err(|error| KernelError::context("Failed to stop plugins", error))?;
            plugin_manager
                .unload_all_plugins()
                .await
                .map_err(|error| KernelError::context("Failed to unload plugins", error))?;
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

    #[tokio::test]
    async fn test_initialize_respects_optional_subsystems() {
        let mut config = KernelConfig::default();
        config.agent_loop.enabled = false;
        config.memory.enabled = false;

        let kernel = AgentKernel::new(config);
        kernel.initialize().await.unwrap();

        assert!(kernel.llm_provider().is_some());
        assert!(kernel.mcp_bridge().is_some());
        assert!(kernel.channel_manager().is_some());
        assert!(kernel.agent_loop().is_none());
        assert!(kernel.memory_system().is_none());
    }

    #[tokio::test]
    async fn test_kernel_startup_keeps_gateway_plugin_available() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root should exist")
            .to_path_buf();
        let mut config = KernelConfig::default();
        config.workspace_root = workspace_root.clone();
        config.plugins_dir = workspace_root.join("plugins");
        let kernel = AgentKernel::new(config);

        kernel.initialize().await.unwrap();

        let plugin_manager = kernel.plugin_manager.get().expect("plugin manager should initialize");
        let gateway = plugin_manager
            .plugin_status("gateway")
            .await
            .expect("gateway plugin should be discovered");
        assert_eq!(gateway.state, plugin_system::PluginState::Discovered);

        kernel.start().await.unwrap();

        let gateway = plugin_manager
            .plugin_status("gateway")
            .await
            .expect("gateway plugin should remain tracked after startup");
        assert_eq!(gateway.state, plugin_system::PluginState::Running);

        kernel.stop().await.unwrap();

        let gateway = plugin_manager
            .plugin_status("gateway")
            .await
            .expect("gateway plugin should remain tracked after shutdown");
        assert_eq!(gateway.state, plugin_system::PluginState::Unloaded);
    }
}
