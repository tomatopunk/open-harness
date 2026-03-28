//! Checkpoint record construction for graph commits (storage backends implement [`agent_ports::CheckpointPort`]).

use agent_ports::{
    CheckpointId, CheckpointRecord, EngineCheckpointExtensions, RunId, ThreadId, ThreadState,
    CHECKPOINT_RECORD_SCHEMA_VERSION,
};

/// Build a checkpoint record wrapping thread state.
#[must_use]
pub fn make_checkpoint(
    thread_id: ThreadId,
    run_id: RunId,
    state: ThreadState,
    metadata: serde_json::Value,
) -> CheckpointRecord {
    let step_seq = state.step_seq;
    let resume_cursor = state
        .pregel
        .interrupt
        .as_ref()
        .and_then(|snap| serde_json::to_string(&snap.resume_cursor).ok());
    let engine = EngineCheckpointExtensions {
        state_schema_version: state.state_schema_version,
        pending_writes_count: state.pregel.pending_write_queue.len() as u32,
        resume_cursor,
        pending_state_effects: Vec::new(),
        superstep_seq: state.pregel.superstep_seq,
    };
    CheckpointRecord {
        record_schema_version: CHECKPOINT_RECORD_SCHEMA_VERSION,
        id: CheckpointId::new_v4(),
        thread_id,
        run_id,
        step_seq,
        state,
        metadata,
        engine,
    }
}

#[cfg(test)]
mod tests {
    use super::make_checkpoint;
    use agent_ports::{
        InterruptKind, InterruptSnapshot, ResumeCursor, StepSeq, ThreadId, ThreadState,
    };
    use serde_json::json;

    #[test]
    fn checkpoint_engine_serializes_resume_cursor_when_interrupt_pending() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        st.pregel.interrupt = Some(InterruptSnapshot {
            kind: InterruptKind::Clarification { prompt: None },
            resume_cursor: ResumeCursor {
                node_id: "dispatch_clarify".into(),
                superstep_seq: 2,
                step_seq: StepSeq::initial(),
            },
            payload: json!({}),
        });
        let cp = make_checkpoint(tid, agent_ports::RunId::default(), st, json!({}));
        assert!(cp.engine.resume_cursor.is_some());
        assert!(cp.engine.resume_cursor.as_ref().unwrap().contains("dispatch_clarify"));
    }
}
