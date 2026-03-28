//! Pregel channel bump on the thread state.

use agent_loop_runtime::pregel::{bump_after_node, CH_STATE};
use agent_ports::ThreadId;

#[test]
fn bump_after_node_increments_channel_and_versions_seen() {
    let tid = ThreadId::new_v4();
    let mut st = agent_ports::ThreadState::new(tid);
    bump_after_node(&mut st, "n1", None);
    assert_eq!(st.pregel.channel_versions.get(CH_STATE), Some(&1));
    assert_eq!(st.pregel.versions_seen.get("n1").and_then(|m| m.get(CH_STATE)), Some(&1u64));
    assert_eq!(st.pregel.pending_write_queue.len(), 1);
    assert_eq!(st.pregel.pending_write_queue[0].node_id, "n1");
    assert!(st.pregel.pending_write_queue[0].task_id.is_none());
}
