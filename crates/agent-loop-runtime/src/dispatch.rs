//! Post-model dispatch: maps [`agent_ports::EngineCommand`] to checkpoints and side effects.

use crate::budget::RunBudget;
use crate::child_run::subagent_params_for_child_run;
use crate::commit_metadata;
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::loop_common::commit_at_stage;
use crate::middleware::TurnContext;
use crate::premodel_phase::run_premodel_skills_memory;
use crate::runtime_spec::{DispatchPhaseNodes, SubagentRuntimeSpec};
use crate::state_patch::StatePatch;
use crate::superstep_kernel::apply_writes_after_node;
use crate::superstep_kernel::execute::{
    invoke_tool_calls_in_call_order, verify_tool_push_tail_matches,
};
use crate::superstep_kernel::prepare::{
    prepare_pull_task, prepare_subagent_fanout, prepare_tool_fanout,
};
use crate::turn_reducer::{apply_turn_effects, tool_round_from_calls, TurnEffect};
use agent_ports::{
    tool_allowed, AgentEvent, DispatchPlan, EngineCommand, EventSink, InterruptKind,
    InterruptSnapshot, LoopStage, ResumeCursor, StepKind, ThreadId, ToolCallSpec, ToolManifest,
};
use graph_runtime_core::GraphRuntime;
use serde_json::{json, Value};
use tracing::warn;

use crate::agent_loop_types::AgentLoopDeps;
use crate::budget::truncate_subtask_plan;
use crate::run_config::AgentLoopRunConfig;

/// Outer loop control after one post-model step completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnDispatch {
    Stop,
    Again,
}

/// Single execution surface for the post-model dispatch phase (Command IR → tasks → writes → commit).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_dispatch_plan(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    turn_ctx: &TurnContext,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    state: &mut agent_ports::ThreadState,
    sink: &mut EventSink,
    manifests: &[ToolManifest],
    plan: DispatchPlan,
    out: &agent_ports::LlmTurnOutput,
    budget: RunBudget,
) -> AgentLoopResult<TurnDispatch> {
    execute_engine_command(
        graph,
        deps,
        turn_ctx,
        thread_id,
        run_id,
        step_seq,
        state,
        sink,
        manifests,
        plan.command,
        out,
        budget,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_engine_command(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    turn_ctx: &TurnContext,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    state: &mut agent_ports::ThreadState,
    sink: &mut EventSink,
    manifests: &[ToolManifest],
    cmd: EngineCommand,
    _out: &agent_ports::LlmTurnOutput,
    budget: RunBudget,
) -> AgentLoopResult<TurnDispatch> {
    match cmd {
        EngineCommand::Interrupt { kind } => match kind {
            InterruptKind::Clarification { prompt } => {
                apply_turn_effects(
                    state,
                    &[TurnEffect::SetClarification { prompt: prompt.clone() }],
                );
                sink.push(AgentEvent::ClarificationRequested { run_id, prompt: prompt.clone() });
                state.pregel.interrupt = Some(InterruptSnapshot {
                    kind: InterruptKind::Clarification { prompt: prompt.clone() },
                    resume_cursor: ResumeCursor {
                        node_id: DispatchPhaseNodes::CLARIFY.into(),
                        superstep_seq: state.pregel.superstep_seq,
                        step_seq: state.step_seq,
                    },
                    payload: json!({}),
                });
                apply_writes_after_node(state, DispatchPhaseNodes::CLARIFY);
                *state = commit_at_stage(
                    graph,
                    thread_id,
                    run_id,
                    state.clone(),
                    sink,
                    LoopStage::ClarifyExit,
                    commit_metadata::clarify_exit(),
                )
                .await?;
                sink.push(AgentEvent::RunCompleted { run_id, reason: "clarification".into() });
                Ok(TurnDispatch::Stop)
            }
        },
        EngineCommand::Subagent { plan, finish_turn } => {
            let (truncated_plan, truncated) = truncate_subtask_plan(plan, &budget);
            if truncated {
                warn!("subtask plan truncated by per-response / budget cap");
            }
            sink.push(AgentEvent::SubagentPlanned {
                run_id,
                task_count: truncated_plan.tasks.len(),
            });

            if turn_ctx.run_cfg.subagent_spec.inherit_premodel_skills_memory {
                prepare_pull_task(state, SubagentRuntimeSpec::NODE_PREMODEL);
                run_premodel_skills_memory(
                    deps,
                    turn_ctx,
                    thread_id,
                    run_id,
                    step_seq,
                    state,
                    sink,
                    &turn_ctx.run_cfg,
                    true,
                )
                .await?;
                apply_writes_after_node(state, SubagentRuntimeSpec::NODE_PREMODEL);
            }

            prepare_subagent_fanout(state, truncated_plan.tasks.len());

            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::SubagentExec, true);
            let sub_params = subagent_params_for_child_run(budget, &turn_ctx.run_cfg.subagent_spec);
            let results = deps
                .subagents
                .execute_plan(run_id, thread_id, &truncated_plan, state, &sub_params, sink)
                .await?;
            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::SubagentExec, false);

            let mut records = Vec::with_capacity(results.len());
            for (i, r) in results.iter().enumerate() {
                sink.push(AgentEvent::SubagentCompleted { run_id, task_id: r.task_id });
                let goal = truncated_plan.tasks.get(i).map(|t| t.goal.clone()).unwrap_or_default();
                records.push(agent_ports::SubagentTaskRecord {
                    task_id: r.task_id,
                    goal,
                    status: if r.ok { "ok".into() } else { "failed".into() },
                    output: Some(r.output.clone()),
                });
            }
            apply_turn_effects(state, &[TurnEffect::AppendSubagentRecords { records }]);

            let merged = deps
                .subagents
                .merge(
                    &agent_ports::SubagentMergeContext { thread_id, run_id, state: state.clone() },
                    &results,
                )
                .await?;
            crate::turn_reducer::merge_subagent_port_into_parent(state, merged);

            apply_writes_after_node(state, DispatchPhaseNodes::SUBAGENT);

            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
            *state = commit_at_stage(
                graph,
                thread_id,
                run_id,
                state.clone(),
                sink,
                LoopStage::StateCommit,
                commit_metadata::state_commit_after_subagent(truncated_plan.tasks.len(), truncated),
            )
            .await?;
            crate::loop_common::emit_stage(
                sink,
                run_id,
                state.step_seq,
                LoopStage::StateCommit,
                false,
            );

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
        EngineCommand::ToolCalls { calls, finish_turn } => {
            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::ToolExec, true);
            sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: StepKind::Tool });

            #[derive(Debug)]
            enum ToolSlot {
                Denied { reason: String },
                Invoke { idx: usize },
            }

            let mut invoke_batch: Vec<ToolCallSpec> = Vec::new();
            let mut slots: Vec<ToolSlot> = Vec::with_capacity(calls.len());

            for call in &calls {
                if !tool_allowed(&call.name, manifests) {
                    warn!(tool = %call.name, "tool not in assembled manifest; recording structured denial");
                    slots.push(ToolSlot::Denied {
                        reason: format!("tool not in assembled manifest: {}", call.name),
                    });
                } else {
                    sink.push(AgentEvent::ToolSelected { run_id, tool_name: call.name.clone() });
                    let idx = invoke_batch.len();
                    invoke_batch.push(call.clone());
                    slots.push(ToolSlot::Invoke { idx });
                }
            }

            prepare_tool_fanout(state, &invoke_batch);
            verify_tool_push_tail_matches(state, &invoke_batch)
                .map_err(crate::error::AgentLoopError::InvariantViolation)?;

            for call in &invoke_batch {
                deps.middleware.before_tool_call(turn_ctx, state, call).await?;
            }

            let max_c = budget.max_concurrent_tool_calls.max(1) as usize;
            let invoked = invoke_tool_calls_in_call_order(
                deps.tools.clone(),
                run_id,
                thread_id,
                &invoke_batch,
                max_c,
            )
            .await;

            let mut payloads: Vec<Result<Value, String>> = Vec::with_capacity(calls.len());

            for slot in slots {
                match slot {
                    ToolSlot::Denied { reason } => {
                        payloads.push(Err(reason));
                    }
                    ToolSlot::Invoke { idx } => {
                        payloads.push(
                            invoked
                                .get(idx)
                                .cloned()
                                .unwrap_or_else(|| Err("internal: missing invoke slot".into())),
                        );
                    }
                }
            }

            let mut tool_names: Vec<String> = Vec::with_capacity(calls.len());
            for (i, call) in calls.iter().enumerate() {
                let payload_res = &payloads[i];
                let ok = payload_res.is_ok();
                let payload =
                    payload_res.as_ref().map_or_else(|e| json!({ "error": e }), |v| v.clone());
                deps.middleware.after_tool_call(turn_ctx, state, call, ok, &payload).await?;
                sink.push(AgentEvent::ToolExecuted { run_id, tool_name: call.name.clone(), ok });
                tool_names.push(call.name.clone());
            }

            let effect = tool_round_from_calls(&calls, payloads);
            StatePatch::from(vec![effect]).apply(state);

            apply_writes_after_node(state, DispatchPhaseNodes::TOOLS);

            sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: StepKind::Tool });
            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::ToolExec, false);

            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
            *state = commit_at_stage(
                graph,
                thread_id,
                run_id,
                state.clone(),
                sink,
                LoopStage::StateCommit,
                commit_metadata::state_commit_after_tools(&tool_names),
            )
            .await?;
            crate::loop_common::emit_stage(
                sink,
                run_id,
                state.step_seq,
                LoopStage::StateCommit,
                false,
            );

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
        EngineCommand::TextAndMemory { assistant_text, finish_turn } => {
            if let Some(text) = assistant_text {
                apply_turn_effects(state, &[TurnEffect::AppendAssistantMessage { text }]);
            }

            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::MemoryCommit, true);
            let _mem = deps.memory.extract_and_commit(state).await?;
            sink.push(AgentEvent::MemoryUpdated);
            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::MemoryCommit, false);

            apply_writes_after_node(state, DispatchPhaseNodes::TEXT);

            crate::loop_common::emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
            *state = commit_at_stage(
                graph,
                thread_id,
                run_id,
                state.clone(),
                sink,
                LoopStage::StateCommit,
                commit_metadata::state_commit_after_memory_turn(),
            )
            .await?;
            crate::loop_common::emit_stage(
                sink,
                run_id,
                state.step_seq,
                LoopStage::StateCommit,
                false,
            );

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
    }
}

/// Build validated dispatch plan from one model turn (Command IR single entry; no silent fallback).
pub(crate) fn route_llm_output(
    out: &agent_ports::LlmTurnOutput,
    run_cfg: &AgentLoopRunConfig,
) -> AgentLoopResult<DispatchPlan> {
    agent_ports::build_dispatch_plan_with_options(out, &run_cfg.dispatch_plan_options())
        .map_err(|reason| AgentLoopError::InvariantViolation(format!("dispatch_plan: {reason}")))
}
