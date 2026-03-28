//! One **outer superstep**（一次 `run_agent_loop` 迭代）：`prepare_tasks` → PULL 相位执行 →
//! [`crate::dispatch::execute_dispatch_plan`]（Command IR / LangGraph 式 execute 面）。
//!
//! 编排集中在此模块，[`crate::superstep_kernel`] 提供 `prepare_tasks` / `apply_writes_after_node` / 工具执行。

use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
use crate::dispatch::{execute_dispatch_plan, route_llm_output, TurnDispatch};
use crate::error::AgentLoopResult;
use crate::lead_kernel::apply_lead_kernel_turn;
use crate::loop_common::emit_stage;
use crate::loop_hardening::{apply_repeated_tool_loop_breaker, repair_missing_tool_results};
use crate::middleware::TurnContext;
use crate::premodel_phase::run_premodel_skills_memory;
use crate::run_config::AgentLoopRunConfig;
use crate::superstep_kernel::prepare::prepare_pull_task;
use crate::superstep_kernel::{apply_writes_after_node, prepare_tasks};
use agent_ports::{AgentEvent, EventSink, LlmTurnContext, LoopStage, ThreadId, ThreadState};
use graph_runtime_core::GraphRuntime;
use serde_json::json;
use tracing::debug;

/// Run one full inner superstep: Lead → PreModel → Model → PostModel → [`execute_dispatch_plan`].
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_inner_superstep_turn(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    turn_ctx: TurnContext,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    state: &mut ThreadState,
    sink: &mut EventSink,
    tool_cfg: &ToolLoopConfig,
    run_cfg: &AgentLoopRunConfig,
    last_tool_call_fingerprint: &mut Option<u64>,
) -> AgentLoopResult<TurnDispatch> {
    // --- prepare_tasks：清空 staged_tasks，开始本回合超步 ---
    prepare_tasks(state);

    // --- Node: Lead ---
    let task_lead = prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_LEAD);
    if run_cfg.lead_spec.apply_lead_kernel {
        apply_lead_kernel_turn(deps.lead_kernel.clone(), state, run_cfg).await?;
    }
    apply_writes_after_node(
        state,
        crate::runtime_spec::LeadRuntimeSpec::NODE_LEAD,
        Some(task_lead),
    );

    repair_missing_tool_results(state);

    // --- Node: PreModel ---
    let task_premodel =
        prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_PREMODEL);
    let injection = run_premodel_skills_memory(
        deps,
        &turn_ctx,
        thread_id,
        run_id,
        step_seq,
        state,
        sink,
        run_cfg,
        run_cfg.lead_spec.premodel_skills_memory,
    )
    .await?;
    apply_writes_after_node(
        state,
        crate::runtime_spec::LeadRuntimeSpec::NODE_PREMODEL,
        Some(task_premodel),
    );

    let manifests = deps.tools.assemble(&tool_cfg.assembly);
    let assembled_tool_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
    let mut messages_for_llm = build_llm_messages(state, &injection.preamble, run_cfg);

    deps.middleware.before_model(&turn_ctx, state, &mut messages_for_llm).await?;

    // --- Node: Model ---
    let _task_model = prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_MODEL);
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
    let task_postmodel =
        prepare_pull_task(state, crate::runtime_spec::LeadRuntimeSpec::NODE_POSTMODEL);
    emit_stage(sink, run_id, step_seq, LoopStage::PostModel, true);
    deps.middleware.after_model(&turn_ctx, state, &out).await?;
    emit_stage(sink, run_id, step_seq, LoopStage::PostModel, false);
    apply_writes_after_node(
        state,
        crate::runtime_spec::LeadRuntimeSpec::NODE_POSTMODEL,
        Some(task_postmodel),
    );

    apply_repeated_tool_loop_breaker(&mut out, last_tool_call_fingerprint);

    sink.push(AgentEvent::LlmCompleted { run_id });
    sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: agent_ports::StepKind::Llm });
    emit_stage(sink, run_id, step_seq, LoopStage::Model, false);

    let plan = route_llm_output(&out, run_cfg)?;
    execute_dispatch_plan(
        graph,
        deps,
        &turn_ctx,
        thread_id,
        run_id,
        step_seq,
        state,
        sink,
        &manifests,
        plan,
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
