use crate::agent_loop::AgentLoop;
use crate::channel_manager::ChannelManager;
use crate::config::KernelConfig;
use crate::events::{EventBus, KernelStartedEvent, KernelStoppedEvent};
use crate::hooks::HookSystem;
use crate::lifecycle::LifecycleManager;
use crate::security::RuntimeSecurityChain;
use crate::state_machine::{
    KernelEvent, KernelSideEffect, KernelState, KernelStateMachine, KernelTransition,
};
use crate::{KernelError, KernelResult};
use agent_ports::StreamingToolRuntime;
use llm_providers::{create_provider, LLMProvider};
use mcp_bridge::McpBridgeManager;
use plugin_system::PluginManager;
use state_abstraction::memory_system::{MemorySystem, MemorySystemConfig};
use state_abstraction::traits::MemoryStore;
use state_abstraction::{
    CreateSessionRequest, ForkSessionRequest, LocalFsStateStore, SandboxExecutionStore,
    SessionCore, SessionRecord,
};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::sync::RwLock;
use uuid::Uuid;

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
    session_core: Arc<SessionCore>,
    channel_manager: Arc<OnceCell<ChannelManager>>,
    tool_runtime: Arc<StreamingToolRuntime>,
    security_chain: Arc<RuntimeSecurityChain>,
    execution_audit_store: Arc<dyn SandboxExecutionStore>,
    state: Arc<RwLock<KernelState>>,
}

impl AgentKernel {
    /// 创建新的 kernel 实例
    pub fn new(config: KernelConfig) -> Self {
        let execution_audit_store = build_execution_audit_store(&config);
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
            session_core: Arc::new(SessionCore::new()),
            channel_manager: Arc::new(OnceCell::new()),
            tool_runtime: Arc::new(StreamingToolRuntime::new()),
            security_chain: Arc::new(RuntimeSecurityChain::new()),
            execution_audit_store,
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

    pub fn session_core(&self) -> &Arc<SessionCore> {
        &self.session_core
    }

    pub(crate) fn tool_runtime(&self) -> &Arc<StreamingToolRuntime> {
        &self.tool_runtime
    }

    pub(crate) fn execution_audit_store(&self) -> &Arc<dyn SandboxExecutionStore> {
        &self.execution_audit_store
    }

    pub(crate) fn security_chain(&self) -> &Arc<RuntimeSecurityChain> {
        &self.security_chain
    }

    /// 获取生命周期管理器
    pub fn lifecycle_manager(&self) -> &Arc<LifecycleManager> {
        &self.lifecycle_manager
    }

    /// 获取配置
    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    pub(crate) async fn transition_tool_execution(&self, event: KernelEvent) -> KernelResult<()> {
        let mut state = self.state.write().await;
        self.apply_transition(&mut state, event).await
    }

    pub(crate) async fn current_state(&self) -> KernelState {
        *self.state.read().await
    }

    pub async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> KernelResult<SessionRecord> {
        self.session_core.create_session(request).await.map_err(Into::into)
    }

    pub async fn attach_session(&self, session_id: Uuid) -> KernelResult<SessionRecord> {
        self.session_core.attach_session(session_id).await.map_err(Into::into)
    }

    pub async fn fork_session(&self, request: ForkSessionRequest) -> KernelResult<SessionRecord> {
        self.session_core.fork_session(request).await.map_err(Into::into)
    }

    pub async fn close_session(&self, session_id: Uuid) -> KernelResult<SessionRecord> {
        self.session_core.close_session(session_id).await.map_err(Into::into)
    }

    async fn apply_transition(
        &self,
        state: &mut KernelState,
        event: KernelEvent,
    ) -> KernelResult<()> {
        let transition = KernelStateMachine::transition(*state, event)?;
        self.apply_side_effects(&transition).await?;
        *state = transition.to;
        Ok(())
    }

    async fn apply_side_effects(&self, transition: &KernelTransition) -> KernelResult<()> {
        for side_effect in &transition.side_effects {
            self.apply_side_effect(side_effect).await?;
        }
        Ok(())
    }

    async fn apply_side_effect(&self, side_effect: &KernelSideEffect) -> KernelResult<()> {
        match side_effect {
            KernelSideEffect::InitializeProviderAdapter => self.initialize_provider_adapter(),
            KernelSideEffect::InitializeMcpAdapter => self.initialize_mcp_adapter().await,
            KernelSideEffect::InitializePluginAdapter => self.initialize_plugin_adapter().await,
            KernelSideEffect::InitializeLoopStateAdapter => self.initialize_loop_state_adapter(),
            KernelSideEffect::InitializeChannelAdapter => self.initialize_channel_adapter(),
            KernelSideEffect::RunLifecycleStages(stages) => {
                self.lifecycle_manager.run_stages(stages).await
            }
            KernelSideEffect::LoadPlugins => {
                if let Some(plugin_manager) = self.plugin_manager.get() {
                    plugin_manager
                        .load_all_plugins()
                        .await
                        .map_err(|error| KernelError::context("Failed to load plugins", error))?;
                }
                Ok(())
            }
            KernelSideEffect::InitializePlugins => {
                if let Some(plugin_manager) = self.plugin_manager.get() {
                    plugin_manager.initialize_all_plugins().await.map_err(|error| {
                        KernelError::context("Failed to initialize plugins", error)
                    })?;
                }
                Ok(())
            }
            KernelSideEffect::StartPlugins => {
                if let Some(plugin_manager) = self.plugin_manager.get() {
                    plugin_manager
                        .start_all_plugins()
                        .await
                        .map_err(|error| KernelError::context("Failed to start plugins", error))?;
                }
                Ok(())
            }
            KernelSideEffect::StartChannels => {
                if let Some(channel_manager) = self.channel_manager.get() {
                    channel_manager.start_all().await.map_err(|error| {
                        KernelError::ExternalConnection(format!(
                            "Failed to start channels: {}",
                            error
                        ))
                    })?;
                }
                Ok(())
            }
            KernelSideEffect::PublishKernelStarted => {
                self.event_bus.publish(KernelStartedEvent).await?;
                Ok(())
            }
            KernelSideEffect::PublishKernelStopped => {
                self.event_bus.publish(KernelStoppedEvent).await?;
                Ok(())
            }
            KernelSideEffect::StopChannels => {
                if let Some(channel_manager) = self.channel_manager.get() {
                    channel_manager.stop_all().await.map_err(|error| {
                        KernelError::Lifecycle(format!("Failed to stop channels: {}", error))
                    })?;
                }
                Ok(())
            }
            KernelSideEffect::StopPlugins => {
                if let Some(plugin_manager) = self.plugin_manager.get() {
                    plugin_manager
                        .stop_all_plugins()
                        .await
                        .map_err(|error| KernelError::context("Failed to stop plugins", error))?;
                }
                Ok(())
            }
            KernelSideEffect::UnloadPlugins => {
                if let Some(plugin_manager) = self.plugin_manager.get() {
                    plugin_manager
                        .unload_all_plugins()
                        .await
                        .map_err(|error| KernelError::context("Failed to unload plugins", error))?;
                }
                Ok(())
            }
            KernelSideEffect::RecordToolExecutionStart { session_id, tool_call_id, tool_name } => {
                tracing::debug!(
                    %session_id,
                    tool_call_id = %tool_call_id,
                    tool_name = %tool_name,
                    "Kernel state machine recorded tool execution start"
                );
                Ok(())
            }
            KernelSideEffect::RecordToolExecutionFinish {
                session_id,
                tool_call_id,
                tool_name,
                success,
            } => {
                tracing::debug!(
                    %session_id,
                    tool_call_id = %tool_call_id,
                    tool_name = %tool_name,
                    success = *success,
                    "Kernel state machine recorded tool execution finish"
                );
                Ok(())
            }
        }
    }

    /// 初始化 kernel
    pub async fn initialize(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;

        tracing::info!("Initializing Open Harness Agent Kernel...");

        self.apply_transition(&mut state, KernelEvent::Initialize).await?;
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
            compression_token_threshold: self.config.memory.compression_token_threshold,
            milestone_snapshot_interval: self.config.memory.milestone_snapshot_interval,
            recent_fact_window: self.config.memory.recent_fact_window,
            working_fact_window: self.config.memory.working_fact_window,
            archived_retrieval_limit: self.config.memory.archived_retrieval_limit,
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

    /// 启动 kernel
    pub async fn start(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;

        tracing::info!("Starting Open Harness Agent Kernel...");

        self.apply_transition(&mut state, KernelEvent::Start).await?;
        tracing::info!("Kernel started successfully");

        Ok(())
    }

    /// 停止 kernel
    pub async fn stop(&self) -> KernelResult<()> {
        let mut state = self.state.write().await;

        tracing::info!("Stopping Open Harness Agent Kernel...");

        self.apply_transition(&mut state, KernelEvent::Stop).await?;
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

fn build_execution_audit_store(config: &KernelConfig) -> Arc<dyn SandboxExecutionStore> {
    let local_fs_root = config
        .storage
        .local_fs
        .as_ref()
        .map(|local_fs| local_fs.root.clone())
        .unwrap_or_else(|| std::path::PathBuf::from(".data/local-fs"));

    Arc::new(LocalFsStateStore::new(config.workspace_root.join(local_fs_root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::LifecycleStage;
    use crate::{KernelEvent, KernelGuard};
    use crate::{SessionContext, SessionPolicy};
    use async_trait::async_trait;
    use plugin_system::{BasePlugin, Plugin, PluginContext, PluginError, PluginLifecycleStage};
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Mutex;

    struct RecordingHook {
        stages: Arc<Mutex<Vec<LifecycleStage>>>,
    }

    #[async_trait]
    impl crate::LifecycleHook for RecordingHook {
        fn name(&self) -> &'static str {
            "RecordingHook"
        }

        async fn on_lifecycle(&self, stage: LifecycleStage) -> crate::KernelResult<()> {
            self.stages.lock().await.push(stage);
            Ok(())
        }
    }

    struct FailingInitializePlugin {
        base: BasePlugin,
    }

    impl FailingInitializePlugin {
        fn new(manifest: plugin_system::PluginManifest) -> Self {
            Self { base: BasePlugin::new(manifest) }
        }
    }

    #[async_trait]
    impl Plugin for FailingInitializePlugin {
        fn manifest(&self) -> &plugin_system::PluginManifest {
            self.base.manifest()
        }

        fn state(&self) -> plugin_system::PluginState {
            self.base.state()
        }

        async fn load(&mut self, ctx: &PluginContext) -> plugin_system::PluginResult<()> {
            self.base.load(ctx).await
        }

        async fn initialize(&mut self, _ctx: &PluginContext) -> plugin_system::PluginResult<()> {
            Err(PluginError::InitializationFailed("intentional initialize failure".to_string()))
        }

        async fn start(&mut self, ctx: &PluginContext) -> plugin_system::PluginResult<()> {
            self.base.start(ctx).await
        }

        async fn stop(&mut self, ctx: &PluginContext) -> plugin_system::PluginResult<()> {
            self.base.stop(ctx).await
        }

        async fn unload(&mut self, ctx: &PluginContext) -> plugin_system::PluginResult<()> {
            self.base.unload(ctx).await
        }
    }

    fn create_plugin_dir(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("agent-kernel-tests-{name}-{now}-{suffix}"));
        let plugin_dir = root.join(name);

        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.yaml"),
            format!(
                "name: {name}\nversion: 0.1.0\ndescription: test plugin\nauthors:\n  - test\ntype: generic\nenabled: true\ndependencies: []\n"
            ),
        )
        .unwrap();

        root
    }

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
    async fn test_kernel_state_machine_rejects_invalid_start_transition() {
        let kernel = AgentKernel::new(KernelConfig::default());
        let error = kernel.start().await.expect_err("start should fail before initialize");

        match error {
            KernelError::StateMachine { source } => {
                assert_eq!(source.from, KernelState::Created);
                assert_eq!(source.event, KernelEvent::Start);
                assert_eq!(source.guard, KernelGuard::StateIs(KernelState::Initialized));
            }
            other => panic!("unexpected error: {other:?}"),
        }
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
    async fn test_initialize_runs_refactor_lifecycle_stages_in_order() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root should exist")
            .to_path_buf();
        let config = KernelConfig {
            workspace_root: workspace_root.clone(),
            plugins_dir: workspace_root.join("plugins"),
            ..Default::default()
        };

        let kernel = AgentKernel::new(config);
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let hook = Arc::new(RecordingHook { stages: recorded.clone() });

        for stage in [
            LifecycleStage::BeforeInit,
            LifecycleStage::InitConfig,
            LifecycleStage::InitProvider,
            LifecycleStage::InitMcp,
            LifecycleStage::InitPlugin,
            LifecycleStage::InitLoopState,
            LifecycleStage::Init,
            LifecycleStage::AfterInit,
            LifecycleStage::BeforeStart,
            LifecycleStage::Start,
            LifecycleStage::AfterStart,
            LifecycleStage::BeforeStop,
            LifecycleStage::Stop,
            LifecycleStage::AfterStop,
            LifecycleStage::Cleanup,
        ] {
            kernel.lifecycle_manager().register_hook(stage, hook.clone()).await;
        }

        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();
        kernel.stop().await.unwrap();

        assert_eq!(
            *recorded.lock().await,
            vec![
                LifecycleStage::BeforeInit,
                LifecycleStage::InitConfig,
                LifecycleStage::InitProvider,
                LifecycleStage::InitMcp,
                LifecycleStage::InitPlugin,
                LifecycleStage::InitLoopState,
                LifecycleStage::Init,
                LifecycleStage::AfterInit,
                LifecycleStage::BeforeStart,
                LifecycleStage::Start,
                LifecycleStage::AfterStart,
                LifecycleStage::BeforeStop,
                LifecycleStage::Stop,
                LifecycleStage::AfterStop,
                LifecycleStage::Cleanup,
            ]
        );
    }

    #[tokio::test]
    async fn test_kernel_initialization_reports_plugin_failures_with_context() {
        let plugins_dir = create_plugin_dir("failing-plugin");
        let config = KernelConfig {
            workspace_root: plugins_dir.clone(),
            plugins_dir: plugins_dir.clone(),
            ..Default::default()
        };

        let kernel = AgentKernel::new(config);
        kernel.initialize().await.unwrap();

        let plugin_manager = kernel.plugin_manager.get().expect("plugin manager should initialize");
        plugin_manager
            .register_factory("failing-plugin", |manifest| {
                Box::new(FailingInitializePlugin::new(manifest))
            })
            .await;

        let error = kernel.start().await.expect_err("plugin initialize should fail");
        assert_eq!(error.category(), crate::KernelErrorCategory::Initialization);
        assert!(error.to_string().contains("Failed to initialize plugins"));

        match error {
            KernelError::Context { context, source } => {
                assert_eq!(context, "Failed to initialize plugins");
                match *source {
                    KernelError::Plugin { source, .. } => match source {
                        plugin_system::PluginError::LifecycleFailed { plugin, stage, source } => {
                            assert_eq!(plugin, "failing-plugin");
                            assert_eq!(stage, PluginLifecycleStage::Initialize);
                            match source.as_ref() {
                                plugin_system::PluginError::InitializationFailed(details) => {
                                    assert!(details.contains("intentional initialize failure"));
                                }
                                other => panic!("unexpected plugin source: {other:?}"),
                            }
                        }
                        other => panic!("unexpected plugin error: {other:?}"),
                    },
                    other => panic!("unexpected kernel source: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }

        fs::remove_dir_all(plugins_dir).unwrap();
    }

    #[tokio::test]
    async fn test_initialize_reports_missing_memory_backend_with_context() {
        let workspace_root = std::env::temp_dir().join("agent-kernel-missing-backend");
        fs::create_dir_all(&workspace_root).unwrap();

        let mut config = KernelConfig {
            workspace_root: workspace_root.clone(),
            plugins_dir: workspace_root.join("plugins"),
            ..Default::default()
        };
        config.storage.mode = crate::config::StorageMode::Postgres;

        let kernel = AgentKernel::new(config);
        let error = kernel.initialize().await.expect_err("missing backend should fail");

        assert_eq!(error.category(), crate::KernelErrorCategory::Config);
        assert!(error.to_string().contains("Failed to create memory store"));

        match error {
            KernelError::Context { context, source } => {
                assert_eq!(context, "Failed to create memory store");
                match *source {
                    KernelError::State { source, .. } => match source {
                        state_abstraction::StateError::Config(message) => {
                            assert!(message.contains("Postgres"));
                            assert!(message.contains("not implemented"));
                        }
                        other => panic!("unexpected state source: {other}"),
                    },
                    other => panic!("unexpected kernel source: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }

        fs::remove_dir_all(workspace_root).unwrap();
    }

    #[test]
    fn test_build_memory_system_propagates_segmented_memory_config() {
        let workspace_root = std::env::temp_dir().join("agent-kernel-memory-config");
        fs::create_dir_all(&workspace_root).unwrap();

        let mut config = KernelConfig {
            workspace_root: workspace_root.clone(),
            plugins_dir: workspace_root.join("plugins"),
            ..Default::default()
        };
        config.memory.compression_token_threshold = 42;
        config.memory.milestone_snapshot_interval = 7;
        config.memory.recent_fact_window = 2;
        config.memory.working_fact_window = 5;
        config.memory.archived_retrieval_limit = 3;

        let kernel = AgentKernel::new(config);
        let memory_system = kernel.build_memory_system().unwrap();

        assert_eq!(memory_system.config().compression_token_threshold, 42);
        assert_eq!(memory_system.config().milestone_snapshot_interval, 7);
        assert_eq!(memory_system.config().recent_fact_window, 2);
        assert_eq!(memory_system.config().working_fact_window, 5);
        assert_eq!(memory_system.config().archived_retrieval_limit, 3);

        fs::remove_dir_all(workspace_root).unwrap();
    }

    #[tokio::test]
    async fn test_kernel_startup_keeps_gateway_plugin_available() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root should exist")
            .to_path_buf();
        let config = KernelConfig {
            workspace_root: workspace_root.clone(),
            plugins_dir: workspace_root.join("plugins"),
            ..Default::default()
        };
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

    #[tokio::test]
    async fn test_session_core_create_attach_fork_close() {
        let kernel = AgentKernel::new(KernelConfig::default());
        let thread_id = Uuid::new_v4();

        let mut context = SessionContext::new();
        context.insert("channel", json!("manage"));
        context.insert("request_id", json!("req-1"));

        let mut root_policy = SessionPolicy::new();
        root_policy.insert("mode", json!("safe"));
        root_policy.insert("max_iterations", json!(10));

        let root = kernel
            .create_session(CreateSessionRequest {
                attached_thread_id: Some(thread_id),
                context: context.clone(),
                policy: root_policy,
            })
            .await
            .unwrap();

        let attached = kernel.attach_session(root.session_id).await.unwrap();
        assert_eq!(attached.session_id, root.session_id);
        assert_eq!(attached.context, context);

        let mut local_policy = SessionPolicy::new();
        local_policy.insert("max_iterations", json!(3));
        local_policy.insert("sandbox", json!("restricted"));

        let child = kernel
            .fork_session(ForkSessionRequest {
                parent_session_id: root.session_id,
                attached_thread_id: None,
                local_policy: local_policy.clone(),
            })
            .await
            .unwrap();

        assert_eq!(child.parent_session_id, Some(root.session_id));
        assert_eq!(child.attached_thread_id, Some(thread_id));
        assert_eq!(child.context, context);
        assert_eq!(child.local_policy, local_policy);
        assert_eq!(child.policy.get("mode"), Some(&json!("safe")));
        assert_eq!(child.policy.get("max_iterations"), Some(&json!(3)));
        assert_eq!(child.policy.get("sandbox"), Some(&json!("restricted")));

        let parent = kernel.session_core().session(root.session_id).await.unwrap();
        assert_eq!(parent.child_session_ids, vec![child.session_id]);

        let closed = kernel.close_session(child.session_id).await.unwrap();
        assert_eq!(closed.lifecycle_state, state_abstraction::SessionLifecycleState::Closed);
        assert!(closed.closed_at.is_some());

        let attach_error = kernel.attach_session(child.session_id).await.unwrap_err();
        assert_eq!(attach_error.category(), crate::KernelErrorCategory::Runtime);
        assert!(matches!(attach_error, KernelError::State { .. }));
    }

    #[tokio::test]
    async fn test_session_core_rejects_invalid_parent_reference() {
        let kernel = AgentKernel::new(KernelConfig::default());
        let error = kernel
            .fork_session(ForkSessionRequest {
                parent_session_id: Uuid::new_v4(),
                attached_thread_id: None,
                local_policy: SessionPolicy::default(),
            })
            .await
            .unwrap_err();

        assert_eq!(error.category(), crate::KernelErrorCategory::Runtime);
        match error {
            KernelError::State { source, .. } => match source {
                state_abstraction::StateError::NotFound(message) => {
                    assert!(message.contains("parent session"));
                }
                other => panic!("unexpected state source: {other}"),
            },
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
