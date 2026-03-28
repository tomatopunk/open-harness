use agent_adapters::{
    default_echo_manifests, DefaultMemoryAdapter, DefaultSkillAdapter, DefaultSubagentAdapter,
    EchoTool, HeuristicLlmAdapter, MemoryCheckpointAdapter, RegistryToolAdapter,
};
use agent_loop_runtime::{
    run_agent_loop, AgentLoopDeps, AgentLoopRunConfig, RunBudget, ToolLoopConfig,
};
use agent_ports::{CheckpointPort, ThreadId, ThreadState, ToolAssemblyPolicy};
use graph_runtime_core::GraphRuntime;
use serde_json::json;
use std::sync::Arc;
use tool_runtime::ToolRegistry;

#[tokio::test]
async fn inner_loop_completes_with_echo_tool() {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(EchoTool));
    let tools = Arc::new(RegistryToolAdapter::new(Arc::new(reg), default_echo_manifests()));
    let deps = AgentLoopDeps::new(
        Arc::new(HeuristicLlmAdapter::default()),
        tools,
        Arc::new(DefaultMemoryAdapter),
        Arc::new(DefaultSkillAdapter::default()),
        Arc::new(DefaultSubagentAdapter),
    );
    let cp: Arc<dyn CheckpointPort> = Arc::new(MemoryCheckpointAdapter::default());
    let graph = GraphRuntime::new(cp);
    let tid = ThreadId::new_v4();
    let state = ThreadState::new(tid);
    let (final_state, sink) = run_agent_loop(
        &graph,
        &deps,
        tid,
        state,
        vec![json!("tool: echo")],
        RunBudget::default(),
        ToolLoopConfig { assembly: ToolAssemblyPolicy::default() },
        AgentLoopRunConfig::default(),
    )
    .await
    .expect("loop");
    assert!(!final_state.tool_results.is_empty());
    assert!(!sink.events.is_empty());
}
