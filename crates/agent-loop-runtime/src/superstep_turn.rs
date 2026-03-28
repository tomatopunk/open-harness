//! One **outer superstep** (one `run_agent_loop` iteration): `prepare_tasks` → phase pulls → dispatch.
//!
//! All phase nodes follow the same `prepare_pull_task` → work → [`crate::superstep_kernel::apply_writes_after_node`] pattern so execution stays aligned with [`agent_ports::PregelMeta::staged_tasks`].

use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
use crate::dispatch::{execute_engine_command, route_llm_output, TurnDispatch};
use crate::error::AgentLoopResult;
use crate::lead_kernel::apply_lead_kernel_turn;
use crate::loop_common::emit_stage;
use crate::loop_hardening::{apply_repeated_tool_loop_breaker, repair_missing_tool_results};
use crate::middleware::TurnContext;
use crate::run_config::AgentLoopRunConfig;
use crate::superstep_kernel::prepare::prepare_pull_task;
use crate::superstep_kernel::{apply_writes_after_node, prepare_tasks};
use agent_ports::{AgentEvent, EventSink, LlmTurnContext, LoopStage, ThreadId, ThreadState};
use graph_runtime_core::GraphRuntime;
use serde_json::json;
use tracing::debug;

/// Run one full inner superstep: Lead → PreModel → Model → PostModel → [`execute_engine_command`].
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_inner_superstep_turn(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    turn_ctx: &TurnContext,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    state: &mut ThreadState,
    sink: &mut EventSink,
    tool_cfg: &ToolLoopConfig,
    run_cfg: &AgentLoopRunConfig,
    last_tool_call_fingerprint: &mut Option<u64>,
) -> AgentLoopResult<TurnDispatch> {
    prepare_tasks(state);

    // --- Node: Lead ---
    prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_LEAD);
    if run_cfg.lead_spec.apply_lead_kernel {
        apply_lead_kernel_turn(deps.lead_kernel.clone(), state, run_cfg).await?;
    }
    apply_writes_after_node(state, crate::runtime_spec::LeadRuntimeSpec::NODE_LEAD);

    repair_missing_tool_results(state);

    // --- Node: PreModel ---
    prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_PREMODEL);
    emit_stage(sink, run_id, step_seq, LoopStage::PreModel, true);
    sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: agent_ports::StepKind::Llm });

    let mut injection = agent_ports::SkillInjection::default();
    if run_cfg.lead_spec.premodel_skills_memory {
        let skill_names = if run_cfg.skills_globally_enabled {
            run_cfg.enabled_skill_names.clone()
        } else {
            Vec::new()
        };
        injection = deps
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
    }
    emit_stage(sink, run_id, step_seq, LoopStage::PreModel, false);
    apply_writes_after_node(state, crate::runtime_spec::LeadRuntimeSpec::NODE_PREMODEL);

    let manifests = deps.tools.assemble(&tool_cfg.assembly);
    let assembled_tool_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
    let mut messages_for_llm = build_llm_messages(state, &injection.preamble, run_cfg);

    deps.middleware.before_model(turn_ctx, state, &mut messages_for_llm).await?;

    // --- Node: Model ---
    prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_MODEL);
    emit_stage(sink, run_id, step_seq, LoopStage::Model, true);
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

    // --- Node: PostModel ---
    prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_POSTMODEL);
    emit_stage(sink, run_id, step_seq, LoopStage::PostModel, true);
    deps.middleware.after_model(turn_ctx, state, &out).await?;
    emit_stage(sink, run_id, step_seq, LoopStage::PostModel, false);
    apply_writes_after_node(state, crate::runtime_spec::LeadRuntimeSpec::NODE_POSTMODEL);

    apply_repeated_tool_loop_breaker(&mut out, last_tool_call_fingerprint);

    sink.push(AgentEvent::LlmCompleted { run_id });
    sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: agent_ports::StepKind::Llm });
    emit_stage(sink, run_id, step_seq, LoopStage::Model, false);

    let cmd = route_llm_output(&out);
    execute_engine_command(
        graph,
        deps,
        turn_ctx,
        thread_id,
        run_id,
        step_seq,
        state,
        sink,
        &manifests,
        cmd,
        &out,
        turn_ctx.budget,
    )
    .await
}

fn build_llm_messages(
    state: &ThreadState,
    preamble: &str,
    run_cfg: &AgentLoopRunConfig,
) -> Vec<serde_json::Value> {
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
