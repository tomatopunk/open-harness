//! Shared stage emission and checkpoint helpers for the V2 Pregel-style engine.

use crate::error::{AgentLoopError, AgentLoopResult};
use agent_ports::{AgentEvent, EventSink, LoopStage, ThreadId, ThreadState};
use graph_runtime_core::GraphRuntime;
use serde_json::Value;

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
    mut state: ThreadState,
    sink: &mut EventSink,
    stage: LoopStage,
    metadata: Value,
) -> AgentLoopResult<ThreadState> {
    state.pregel.superstep_seq = state.pregel.superstep_seq.saturating_add(1);
    emit_stage(sink, run_id, state.step_seq, stage, true);
    let (committed_seq, mut committed) = graph
        .commit_step(thread_id, run_id, state, metadata)
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    committed.pregel.staged_tasks.clear();
    committed.pregel.pending_write_queue.clear();
    emit_stage(sink, run_id, committed_seq, stage, false);
    sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });
    Ok(committed)
}
