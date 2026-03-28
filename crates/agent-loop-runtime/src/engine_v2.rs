//! Pregel-style superstep orchestration over the agent loop（V2：`superstep_kernel` 三阶段管线）。

use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
use crate::budget::RunBudget;
use crate::dispatch::TurnDispatch;
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::middleware::TurnContext;
use crate::run_config::AgentLoopRunConfig;
use crate::state_reducer::{append_user_messages, apply_run_config_bootstrap};
use crate::superstep_turn::execute_inner_superstep_turn;
use agent_ports::{AgentEvent, EventSink, LoopStage, ThreadId, ThreadState};
use graph_runtime_core::GraphRuntime;
use serde_json::Value;
use tracing::debug;

/// Run the main loop until finish or max turns (V2 Pregel-scheduled engine).
pub async fn run_agent_loop(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    thread_id: ThreadId,
    mut state: ThreadState,
    user_messages: Vec<Value>,
    budget: RunBudget,
    tool_cfg: ToolLoopConfig,
    run_cfg: AgentLoopRunConfig,
) -> AgentLoopResult<(ThreadState, EventSink)> {
    let mut sink = EventSink::default();
    let run = graph
        .start_run(thread_id, state.clone())
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    let run_id = run.run_id;
    sink.push(AgentEvent::RunStarted { thread_id, run_id });

    append_user_messages(&mut state, &user_messages);
    apply_run_config_bootstrap(&mut state, &run_cfg);
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

        debug!(turn = turns, step_seq = ?step_seq, "inner superstep turn");

        match execute_inner_superstep_turn(
            graph,
            deps,
            &turn_ctx_base,
            thread_id,
            run_id,
            step_seq,
            &mut state,
            &mut sink,
            &tool_cfg,
            &run_cfg,
            &mut last_tool_call_fingerprint,
        )
        .await?
        {
            TurnDispatch::Stop => break,
            TurnDispatch::Again => continue,
        }
    }

    let finalize_seq = state.step_seq;
    crate::loop_common::emit_stage(&mut sink, run_id, finalize_seq, LoopStage::Finalize, true);
    let (_handle, state) = graph
        .complete_run(thread_id, run_id, state, "completed")
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    crate::loop_common::emit_stage(&mut sink, run_id, state.step_seq, LoopStage::Finalize, false);
    sink.push(AgentEvent::RunCompleted { run_id, reason: "completed".into() });

    Ok((state, sink))
}
