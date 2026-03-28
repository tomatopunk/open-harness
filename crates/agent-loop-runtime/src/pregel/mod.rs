//! Pregel-style scheduling helpers (channel bump / node seen / pending writes).

use agent_ports::{PendingWriteRecord, ThreadState};
use uuid::Uuid;

/// Primary logical channel for whole-thread snapshots.
pub const CH_STATE: &str = "state";

/// After a node mutates `state`, bump the state channel and record node observation.
///
/// `task_id` is the [`agent_ports::TaskEnvelope::id`] for the schedulable unit that produced this write
/// (LangGraph `task_id` alignment).
#[inline]
pub fn bump_after_node(state: &mut ThreadState, node_id: &str, task_id: Option<Uuid>) {
    let v = state.pregel.bump_channel(CH_STATE);
    state.pregel.mark_node_seen(node_id, CH_STATE, v);
    state.pregel.pending_write_queue.push(PendingWriteRecord {
        channel: CH_STATE.to_string(),
        version: v,
        node_id: node_id.to_string(),
        task_id,
    });
}
