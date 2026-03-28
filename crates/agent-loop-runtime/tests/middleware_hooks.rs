//! Integration: `before_model` / `after_model` / tool hooks mutate or observe state.

use agent_adapters::{
    default_echo_manifests, DefaultMemoryAdapter, DefaultSkillAdapter, DefaultSubagentAdapter,
    EchoTool, HeuristicLlmAdapter, MemoryCheckpointAdapter, RegistryToolAdapter,
};
use agent_loop_runtime::{
    run_agent_loop, AgentLoopDeps, AgentLoopMiddleware, AgentLoopRunConfig, RunBudget,
    ToolLoopConfig, TurnContext,
};
use agent_ports::{
    CheckpointPort, LlmTurnOutput, ThreadId, ThreadState, ToolAssemblyPolicy, ToolCallSpec,
};
use async_trait::async_trait;
use graph_runtime_core::GraphRuntime;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tool_runtime::ToolRegistry;

struct CountingMiddleware {
    pub before: AtomicUsize,
    pub after: AtomicUsize,
    pub before_tool: AtomicUsize,
    pub after_tool: AtomicUsize,
    pub tag_before: AtomicBool,
}

impl CountingMiddleware {
    fn new() -> Self {
        Self {
            before: AtomicUsize::new(0),
            after: AtomicUsize::new(0),
            before_tool: AtomicUsize::new(0),
            after_tool: AtomicUsize::new(0),
            tag_before: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl AgentLoopMiddleware for CountingMiddleware {
    async fn before_model(
        &self,
        _ctx: &TurnContext,
        state: &mut ThreadState,
        _messages_for_llm: &mut Vec<Value>,
    ) -> agent_loop_runtime::AgentLoopResult<()> {
        self.before.fetch_add(1, Ordering::SeqCst);
        state.governance_marks.tags.push("before_model_ran".into());
        self.tag_before.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn after_model(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _out: &LlmTurnOutput,
    ) -> agent_loop_runtime::AgentLoopResult<()> {
        self.after.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _call: &ToolCallSpec,
    ) -> agent_loop_runtime::AgentLoopResult<()> {
        self.before_tool.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _call: &ToolCallSpec,
        _ok: bool,
        _payload: &Value,
    ) -> agent_loop_runtime::AgentLoopResult<()> {
        self.after_tool.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn middleware_hooks_fire_on_tool_path() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(EchoTool));
    let tools = Arc::new(RegistryToolAdapter::new(Arc::new(reg), default_echo_manifests()));
    let mw = Arc::new(CountingMiddleware::new());
    let mut deps = AgentLoopDeps::new(
        Arc::new(HeuristicLlmAdapter::default()),
        tools,
        Arc::new(DefaultMemoryAdapter),
        Arc::new(DefaultSkillAdapter::default()),
        Arc::new(DefaultSubagentAdapter),
    );
    deps.middleware = mw.clone();
    let cp: Arc<dyn CheckpointPort> = Arc::new(MemoryCheckpointAdapter::default());
    let graph = GraphRuntime::new(cp);
    let tid = ThreadId::new_v4();
    let state = ThreadState::new(tid);
    let (final_state, _sink) = run_agent_loop(
        &graph,
        &deps,
        tid,
        state,
        vec![json!("tool: echo")],
        RunBudget::default(),
        &ToolLoopConfig { assembly: ToolAssemblyPolicy::default() },
        &AgentLoopRunConfig::default(),
    )
    .await
    .expect("loop");

    assert!(
        final_state.governance_marks.tags.iter().any(|t| t == "before_model_ran"),
        "before_model should tag state"
    );
    assert_eq!(mw.before.load(Ordering::SeqCst), 1);
    assert_eq!(mw.after.load(Ordering::SeqCst), 1);
    assert_eq!(mw.before_tool.load(Ordering::SeqCst), 1);
    assert_eq!(mw.after_tool.load(Ordering::SeqCst), 1);
    assert!(mw.tag_before.load(Ordering::SeqCst));
}
