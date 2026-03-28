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
    let engine = EngineCheckpointExtensions {
        state_schema_version: state.state_schema_version,
        pending_writes_count: state.pregel.pending_write_queue.len() as u32,
        resume_cursor: None,
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
