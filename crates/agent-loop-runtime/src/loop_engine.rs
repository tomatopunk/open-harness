//! Multi-turn model-tool-state loop with explicit [`LoopStage`] boundaries.
//!
//! # Stage mapping (see `ARCHITECTURE.md` in this crate)
//!
//! | Stage | When |
//! |-------|------|
//! | [`LoopStage::PreModel`] | After [`crate::lead_kernel::apply_lead_kernel_turn`]: skill injection + memory retrieve. |
//! | [`LoopStage::Model`] | `LLMPort::infer_turn` only. |
//! | [`LoopStage::PostModel`] | `AgentLoopMiddleware::after_model` (no `commit_step` here). |
//! | [`LoopStage::ClarifyExit`] | Pending clarification; checkpoint then return. |
//! | [`LoopStage::SubagentExec`] | Subagent plan execution + merge. |
//! | [`LoopStage::ToolExec`] | Tool invocations for one model turn. |
//! | [`LoopStage::MemoryCommit`] | `MemoryPort::extract_and_commit` on the text path. |
//! | [`LoopStage::StateCommit`] | Checkpoint after a branch mutates durable state. |
//! | [`LoopStage::Finalize`] | `GraphRuntime::complete_run`. |

use crate::budget::RunBudget;
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::execution_kernel::{execute_engine_command, TurnDispatch};
use crate::lead_kernel::apply_lead_kernel_turn;
use crate::loop_hardening::{apply_repeated_tool_loop_breaker, repair_missing_tool_results};
use crate::middleware::{AgentLoopMiddleware, NoopMiddleware, TurnContext};
use crate::run_config::AgentLoopRunConfig;
use crate::state_reducer::{append_user_messages, apply_run_config_bootstrap};
use agent_ports::{
    AgentEvent, EngineCommand, EventSink, LLMPort, LlmTurnContext, LoopStage, MemoryPort, SkillPort,
    SubagentPort, ThreadId, ThreadState, ToolAssemblyPolicy, ToolPort,
};
use graph_runtime_core::GraphRuntime;
use runtime_kernel::RuntimeKernel;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::debug;

/// Bundles ports for one agent loop execution.
pub struct AgentLoopDeps {
    pub llm: Arc<dyn LLMPort>,
    pub tools: Arc<dyn ToolPort>,
    pub memory: Arc<dyn MemoryPort>,
    pub skills: Arc<dyn SkillPort>,
    pub subagents: Arc<dyn SubagentPort>,
    /// Same `before_turn` chain as lead runtime (`LeadPipeline` / DeerFlow-style).
    pub lead_kernel: Arc<RuntimeKernel>,
    /// Optional middleware chain (defaults to no-op).
    pub middleware: Arc<dyn AgentLoopMiddleware>,
}

impl std::fmt::Debug for AgentLoopDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoopDeps")
            .field("llm", &"...")
            .field("tools", &"...")
            .field("memory", &"...")
            .field("skills", &"...")
            .field("subagents", &"...")
            .field("lead_kernel", &"...")
            .field("middleware", &"...")
            .finish()
    }
}

impl AgentLoopDeps {
    /// Build deps without custom middleware (no-op).
    #[must_use]
    pub fn new(
        llm: Arc<dyn LLMPort>,
        tools: Arc<dyn ToolPort>,
        memory: Arc<dyn MemoryPort>,
        skills: Arc<dyn SkillPort>,
        subagents: Arc<dyn SubagentPort>,
    ) -> Self {
        Self {
            llm,
            tools,
            memory,
            skills,
            subagents,
            lead_kernel: Arc::new(RuntimeKernel::default()),
            middleware: Arc::new(NoopMiddleware),
        }
    }
}

/// Configuration for dynamic tool assembly (from governance).
#[derive(Debug, Clone, Default)]
pub struct ToolLoopConfig {
    pub assembly: ToolAssemblyPolicy,
}

pub(crate) fn emit_stage(
    sink: &mut EventSink,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    stage: LoopStage,
    start: bool,
) {
    if start {
        sink.push(AgentEvent::StageStarted { run_id, step_seq, stage });
    } else {
        sink.push(AgentEvent::StageFinished { run_id, step_seq, stage });
    }
}

pub(crate) async fn commit_at_stage(
    graph: &GraphRuntime,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    state: ThreadState,
    sink: &mut EventSink,
    stage: LoopStage,
    metadata: Value,
) -> AgentLoopResult<ThreadState> {
    emit_stage(sink, run_id, state.step_seq, stage, true);
    let (committed_seq, committed) = graph
        .commit_step(thread_id, run_id, state, metadata)
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    emit_stage(sink, run_id, committed_seq, stage, false);
    sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });
    Ok(committed)
}

/// Run the main loop until finish or max turns.
pub async fn run_agent_loop(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    thread_id: ThreadId,
    mut state: ThreadState,
    user_messages: Vec<Value>,
    budget: RunBudget,
    tool_cfg: &ToolLoopConfig,
    run_cfg: &AgentLoopRunConfig,
) -> AgentLoopResult<(ThreadState, EventSink)> {
    let mut sink = EventSink::default();
    let run = graph
        .start_run(thread_id, state.clone())
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    let run_id = run.run_id;
    sink.push(AgentEvent::RunStarted { thread_id, run_id });

    append_user_messages(&mut state, &user_messages);
    apply_run_config_bootstrap(&mut state, run_cfg);
    state.migrate_to_latest_schema();

    let turn_ctx_base = TurnContext { thread_id, run_id, run_cfg: run_cfg.clone(), budget };

    let mut turns: u32 = 0;
    let mut last_tool_call_fingerprint: Option<u64> = None;
    loop {
        if turns >= budget.max_turns {
            return Err(AgentLoopError::MaxTurnsExceeded);
        }
        turns += 1;

        let step_seq = state.step_seq;

        apply_lead_kernel_turn(deps.lead_kernel.as_ref(), &mut state, run_cfg).await?;

        repair_missing_tool_results(&mut state);

        // --- PreModel: skills + memory ---
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PreModel, true);
        sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: agent_ports::StepKind::Llm });

        let skill_names = if run_cfg.skills_globally_enabled {
            run_cfg.enabled_skill_names.clone()
        } else {
            Vec::new()
        };
        let injection = deps
            .skills
            .inject(&agent_ports::SkillContext { thread_id, enabled_skill_names: skill_names })
            .await?;
        if !injection.resolved_names.is_empty() {
            sink.push(AgentEvent::SkillInjected { skill_names: injection.resolved_names.clone() });
        }

        let mem_snippets = deps
            .memory
            .retrieve(&agent_ports::MemoryContext {
                thread_id,
                run_id,
                query: "turn".into(),
                state: state.clone(),
            })
            .await?;
        if !mem_snippets.is_empty() {
            state.memory_working_set.snippets = mem_snippets;
        }
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PreModel, false);

        let manifests = deps.tools.assemble(&tool_cfg.assembly);
        let assembled_tool_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
        let mut messages_for_llm = build_llm_messages(&state, &injection.preamble, run_cfg);

        deps.middleware.before_model(&turn_ctx_base, &mut state, &mut messages_for_llm).await?;

        // --- Model ---
        emit_stage(&mut sink, run_id, step_seq, LoopStage::Model, true);
        let mut out = deps
            .llm
            .infer_turn(LlmTurnContext {
                run_id,
                thread_id,
                messages: messages_for_llm,
                system_prompt: Some(injection.preamble),
                model_name: run_cfg.model_name.clone(),
                policy_version: Some(run_cfg.policy_version.clone()).filter(|s| !s.is_empty()),
                is_plan_mode: run_cfg.is_plan_mode,
                assembled_tool_names,
                loop_detected: run_cfg.loop_detected,
            })
            .await?;

        // --- PostModel (middleware only; no checkpoint) ---
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PostModel, true);
        deps.middleware.after_model(&turn_ctx_base, &mut state, &out).await?;
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PostModel, false);

        apply_repeated_tool_loop_breaker(&mut out, &mut last_tool_call_fingerprint);

        sink.push(AgentEvent::LlmCompleted { run_id });
        sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: agent_ports::StepKind::Llm });
        emit_stage(&mut sink, run_id, step_seq, LoopStage::Model, false);

        let cmd = EngineCommand::from_llm_output(&out);
        match execute_engine_command(
            graph,
            deps,
            &turn_ctx_base,
            thread_id,
            run_id,
            step_seq,
            &mut state,
            &mut sink,
            &manifests,
            cmd,
            &out,
            budget,
        )
        .await?
        {
            TurnDispatch::Stop => break,
            TurnDispatch::Again => continue,
        }
    }

    let finalize_seq = state.step_seq;
    emit_stage(&mut sink, run_id, finalize_seq, LoopStage::Finalize, true);
    let (_handle, state) = graph
        .complete_run(thread_id, run_id, state, "completed")
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    emit_stage(&mut sink, run_id, state.step_seq, LoopStage::Finalize, false);
    sink.push(AgentEvent::RunCompleted { run_id, reason: "completed".into() });

    Ok((state, sink))
}

fn build_llm_messages(
    state: &ThreadState,
    preamble: &str,
    run_cfg: &AgentLoopRunConfig,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut system = String::new();
    if !preamble.is_empty() {
        system.push_str(preamble);
    }
    if run_cfg.is_plan_mode && !system.is_empty() {
        system.push_str("\nPlan mode: maintain an explicit todo list and complete steps in order.");
    } else if run_cfg.is_plan_mode {
        system.push_str("Plan mode: maintain an explicit todo list and complete steps in order.");
    }
    if !system.is_empty() {
        out.push(json!({
            "role": "system",
            "content": system
        }));
    }
    for m in &state.messages {
        out.push(json!({
            "role": m.role,
            "content": m.content
        }));
    }
    debug!(count = out.len(), "llm messages built");
    out
}
