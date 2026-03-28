//! Checkpoint record construction for graph commits (storage backends implement [`agent_ports::CheckpointPort`]).

use agent_ports::{
    CheckpointId, CheckpointRecord, RunId, ThreadId, ThreadState, CHECKPOINT_RECORD_SCHEMA_VERSION,
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
    CheckpointRecord {
        record_schema_version: CHECKPOINT_RECORD_SCHEMA_VERSION,
        id: CheckpointId::new_v4(),
        thread_id,
        run_id,
        step_seq,
        state,
        metadata,
    }
}
