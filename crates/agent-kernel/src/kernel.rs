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

        // 初始化 LLM provider from configuration
        let provider = create_provider(&self.config.llm).map_err(|e| {
            KernelError::Initialization(format!("Failed to create LLM provider: {}", e))
        })?;
        self.llm_provider
            .set(provider)
            .map_err(|_| KernelError::Lifecycle("LLM provider already initialized".to_string()))?;

        // 初始化 MCP bridge
        let mcp_config = mcp_bridge::McpBridgeConfig::default();
        let mcp_bridge = mcp_bridge::McpBridgeManager::new(mcp_config);
        mcp_bridge.initialize().await.map_err(|e| {
            KernelError::Initialization(format!("Failed to initialize MCP bridge: {}", e))
        })?;
        self.mcp_bridge
            .set(mcp_bridge)
            .map_err(|_| KernelError::Lifecycle("MCP bridge already initialized".to_string()))?;

        // 初始化插件管理器
        let plugin_manager = PluginManager::new(self.config.plugins_dir.clone());
        plugin_manager.discover_plugins().await?;
        self.plugin_manager.set(plugin_manager).map_err(|_| {
            KernelError::Lifecycle("Plugin manager already initialized".to_string())
        })?;

        // 初始化 Agent Loop（如果配置启用）
        if self.config.agent_loop.enabled {
            let agent_loop = AgentLoop::new(
                self.config.agent_loop.clone(),
                self.hooks.clone(),
                self.event_bus.clone(),
                self.llm_provider.clone(),
            );
            self.agent_loop.set(agent_loop).map_err(|_| {
                KernelError::Lifecycle("Agent Loop already initialized".to_string())
            })?;
        }

        // 初始化 Memory System（如果配置启用）
        if self.config.memory.enabled {
            let memory_config = MemorySystemConfig {
                max_facts: self.config.memory.max_facts,
                fact_confidence_threshold: self.config.memory.fact_confidence_threshold,
                max_injection_tokens: self.config.memory.max_injection_tokens,
            };
            // Convert agent-kernel config to state-abstraction config
            let storage_config = state_abstraction::StorageConfig {
                mode: match self.config.storage.mode {
                    crate::config::StorageMode::LocalFs => state_abstraction::StorageMode::LocalFs,
                    crate::config::StorageMode::Sqlite => state_abstraction::StorageMode::Sqlite,
                    crate::config::StorageMode::Postgres => {
                        state_abstraction::StorageMode::Postgres
                    }
                },
                local_fs: self
                    .config
                    .storage
                    .local_fs
                    .as_ref()
                    .map(|l| state_abstraction::LocalFsConfig { root: l.root.clone() }),
                sqlite: None,
            };
            let store = state_abstraction::create_memory_store(
                &storage_config,
                &self.config.workspace_root,
                &self.config.memory.storage_path,
            )
            .map_err(|e| {
                KernelError::Initialization(format!("Failed to create memory store: {}", e))
            })?;
            let memory_system = MemorySystem::new(memory_config, store);
            self.memory_system.set(memory_system).map_err(|_| {
                KernelError::Lifecycle("Memory system already initialized".to_string())
            })?;
        }

        // 初始化 Channel Manager
        let channel_manager = ChannelManager::new(self.config.channels.clone());
        self.channel_manager.set(channel_manager).map_err(|_| {
            KernelError::Lifecycle("Channel manager already initialized".to_string())
        })?;

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
        if let Some(plugin_manager) = self.plugin_manager.get() {
            plugin_manager.load_all_plugins().await?;
        }

        // 启动所有渠道
        if let Some(channel_manager) = self.channel_manager.get() {
            channel_manager.start_all().await.map_err(|e| {
                KernelError::Initialization(format!("Failed to start channels: {}", e))
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
