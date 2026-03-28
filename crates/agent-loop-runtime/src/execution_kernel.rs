//! Post-model execution kernel: maps [`agent_ports::EngineCommand`] to checkpoints and side effects.
//!
//! Tool invocations run with bounded concurrency; middleware hooks stay sequential for deterministic state access.

use crate::budget::RunBudget;
use crate::commit_metadata;
use crate::error::AgentLoopResult;
use crate::loop_engine::{commit_at_stage, emit_stage, AgentLoopDeps};
use crate::middleware::TurnContext;
use crate::state_patch::StatePatch;
use crate::turn_reducer::{apply_turn_effects, tool_round_from_calls, TurnEffect};
use agent_ports::{
    tool_allowed, AgentEvent, EngineCommand, EventSink, LoopStage, StepKind, SubagentExecuteParams,
    ThreadId, ToolCallSpec, ToolManifest, ToolPort,
};
use futures::stream::{self, StreamExt};
use graph_runtime_core::GraphRuntime;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::warn;

/// Outer loop control after one post-model step completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnDispatch {
    Stop,
    Again,
}

/// Run parallel tool invokes with a bounded fan-out (middleware before/after stays sequential).
async fn invoke_tools_bounded(
    tools: Arc<dyn ToolPort>,
    run_id: agent_ports::RunId,
    thread_id: ThreadId,
    calls: Vec<ToolCallSpec>,
    max_concurrent: usize,
) -> Vec<Result<Value, String>> {
    stream::iter(calls.into_iter())
        .map(|call| {
            let tools = tools.clone();
            async move { tools.invoke(run_id, thread_id, &call).await.map_err(|e| e.to_string()) }
        })
        .buffer_unordered(max_concurrent.max(1))
        .collect()
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
    out: &agent_ports::LlmTurnOutput,
    budget: RunBudget,
) -> AgentLoopResult<TurnDispatch> {
    match cmd {
        EngineCommand::ClarifyExit => {
            apply_turn_effects(
                state,
                vec![TurnEffect::SetClarification { prompt: out.clarification_prompt.clone() }],
            );
            sink.push(AgentEvent::ClarificationRequested {
                run_id,
                prompt: out.clarification_prompt.clone(),
            });
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
        EngineCommand::Subagent { plan, finish_turn } => {
            let max_t = budget.max_subagent_tasks.max(1) as usize;
            let effective_cap = max_t.min(4);
            let original_len = plan.tasks.len();
            let truncated_plan = agent_ports::SubtaskPlan {
                tasks: plan.tasks.into_iter().take(effective_cap).collect(),
            };
            let truncated = original_len > truncated_plan.tasks.len();
            if truncated {
                warn!("subtask plan truncated by per-response / budget cap");
            }
            sink.push(AgentEvent::SubagentPlanned {
                run_id,
                task_count: truncated_plan.tasks.len(),
            });

            emit_stage(sink, run_id, step_seq, LoopStage::SubagentExec, true);
            let sub_params = SubagentExecuteParams {
                max_concurrent: budget.max_concurrent_subagents.max(1),
                per_task_timeout: Some(std::time::Duration::from_secs(120)),
            };
            let results = deps
                .subagents
                .execute_plan(run_id, thread_id, &truncated_plan, state, &sub_params, sink)
                .await?;
            emit_stage(sink, run_id, step_seq, LoopStage::SubagentExec, false);

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
            apply_turn_effects(state, vec![TurnEffect::AppendSubagentRecords(records)]);

            let merged = deps
                .subagents
                .merge(
                    &agent_ports::SubagentMergeContext { thread_id, run_id, state: state.clone() },
                    &results,
                )
                .await?;
            apply_turn_effects(state, vec![TurnEffect::ReplaceFromMerge(Box::new(merged))]);

            emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
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
            emit_stage(sink, run_id, state.step_seq, LoopStage::StateCommit, false);

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
        EngineCommand::ToolCalls { calls, finish_turn } => {
            emit_stage(sink, run_id, step_seq, LoopStage::ToolExec, true);
            sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: StepKind::Tool });

            let mut allowed: Vec<ToolCallSpec> = Vec::new();
            for call in &calls {
                if !tool_allowed(&call.name, manifests) {
                    warn!("tool not in assembled manifest: {}", call.name);
                    continue;
                }
                sink.push(AgentEvent::ToolSelected { run_id, tool_name: call.name.clone() });
                allowed.push(call.clone());
            }

            for call in &allowed {
                deps.middleware.before_tool_call(turn_ctx, state, call).await?;
            }

            let max_c = budget.max_concurrent_tool_calls.max(1) as usize;
            let payloads =
                invoke_tools_bounded(deps.tools.clone(), run_id, thread_id, allowed.clone(), max_c)
                    .await;

            let mut tool_names: Vec<String> = Vec::with_capacity(allowed.len());
            for i in 0..allowed.len() {
                let call = &allowed[i];
                let payload_res = &payloads[i];
                let ok = payload_res.is_ok();
                let payload = payload_res
                    .as_ref()
                    .map_or_else(|e| json!({ "error": e }), |v| v.clone());
                deps.middleware.after_tool_call(turn_ctx, state, call, ok, &payload).await?;
                sink.push(AgentEvent::ToolExecuted { run_id, tool_name: call.name.clone(), ok });
                tool_names.push(call.name.clone());
            }

            let effect = tool_round_from_calls(&allowed, payloads);
            StatePatch::from(vec![effect]).apply(state);

            sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: StepKind::Tool });
            emit_stage(sink, run_id, step_seq, LoopStage::ToolExec, false);

            emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
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
            emit_stage(sink, run_id, state.step_seq, LoopStage::StateCommit, false);

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
        EngineCommand::TextAndMemory { assistant_text, finish_turn } => {
            if let Some(text) = assistant_text {
                apply_turn_effects(state, vec![TurnEffect::AppendAssistantMessage(text)]);
            }

            emit_stage(sink, run_id, step_seq, LoopStage::MemoryCommit, true);
            let _mem = deps.memory.extract_and_commit(state).await?;
            sink.push(AgentEvent::MemoryUpdated);
            emit_stage(sink, run_id, step_seq, LoopStage::MemoryCommit, false);

            emit_stage(sink, run_id, step_seq, LoopStage::StateCommit, true);
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
            emit_stage(sink, run_id, state.step_seq, LoopStage::StateCommit, false);

            if finish_turn {
                Ok(TurnDispatch::Stop)
            } else {
                Ok(TurnDispatch::Again)
            }
        }
    }
}
