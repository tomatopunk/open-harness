//! Stage events and subagent budget wiring.

use agent_adapters::{
    default_echo_manifests, DefaultMemoryAdapter, DefaultSkillAdapter, DefaultSubagentAdapter,
    EchoTool, HeuristicLlmAdapter, MemoryCheckpointAdapter, RegistryToolAdapter,
};
use agent_loop_runtime::{
    run_agent_loop, AgentLoopDeps, AgentLoopRunConfig, RunBudget, ToolLoopConfig,
};
use agent_ports::{
    AgentEvent, CheckpointPort, LoopStage, ThreadId, ThreadState, ToolAssemblyPolicy,
};
use graph_runtime_core::GraphRuntime;
use serde_json::json;
use std::sync::Arc;
use tool_runtime::ToolRegistry;

fn has_stage(events: &[AgentEvent], stage: LoopStage) -> bool {
    events.iter().any(|e| {
        matches!(e, AgentEvent::StageStarted { stage: s, .. } if *s == stage)
            || matches!(e, AgentEvent::StageFinished { stage: s, .. } if *s == stage)
    })
}

#[tokio::test]
async fn inner_loop_emits_loop_stages() {
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
    let deps = Arc::new(deps);
    let cp: Arc<dyn CheckpointPort> = Arc::new(MemoryCheckpointAdapter::default());
    let graph = Arc::new(GraphRuntime::new(cp));
    let tid = ThreadId::new_v4();
    let state = ThreadState::new(tid);
    let (_final_state, sink) = run_agent_loop(
        Arc::clone(&graph),
        Arc::clone(&deps),
        tid,
        state,
        vec![json!("tool: echo")],
        RunBudget::default(),
        ToolLoopConfig { assembly: ToolAssemblyPolicy::default() },
        AgentLoopRunConfig::default(),
    )
    .await
    .expect("loop");

    assert!(has_stage(&sink.events, LoopStage::PreModel));
    assert!(has_stage(&sink.events, LoopStage::Model));
    assert!(has_stage(&sink.events, LoopStage::PostModel));
    assert!(has_stage(&sink.events, LoopStage::ToolExec));
    assert!(has_stage(&sink.events, LoopStage::Finalize));
}

#[tokio::test]
async fn subagent_plan_respects_concurrency_budget_events() {
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
    let deps = Arc::new(deps);
    let cp: Arc<dyn CheckpointPort> = Arc::new(MemoryCheckpointAdapter::default());
    let graph = Arc::new(GraphRuntime::new(cp));
    let tid = ThreadId::new_v4();
    let state = ThreadState::new(tid);
    let budget = RunBudget {
        max_turns: 8,
        max_subagent_tasks: 8,
        subagent_task_cap_per_response: 4,
        max_concurrent_subagents: 2,
        max_concurrent_tool_calls: 8,
        per_subagent_task_timeout: Some(std::time::Duration::from_secs(120)),
        max_retries_per_task: 3,
        retry_backoff_factor: 2.0,
        max_total_wall_time: None,
        token_budget: None,
        retry_initial_delay_ms: 1000,
        retry_max_delay_ms: 30000,
        retry_jitter: true,
    };
    let (_final_state, sink) = run_agent_loop(
        Arc::clone(&graph),
        Arc::clone(&deps),
        tid,
        state,
        vec![json!("subagent: test goal")],
        budget,
        ToolLoopConfig { assembly: ToolAssemblyPolicy::default() },
        AgentLoopRunConfig::default(),
    )
    .await
    .expect("loop");

    let started =
        sink.events.iter().filter(|e| matches!(e, AgentEvent::SubagentTaskStarted { .. })).count();
    assert_eq!(started, 1);
    assert!(has_stage(&sink.events, LoopStage::SubagentExec));
}
