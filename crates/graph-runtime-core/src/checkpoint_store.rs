//! In-memory checkpoint store (production may swap for durable backend).

use std::collections::HashMap;
use std::sync::Arc;

use agent_ports::{CheckpointId, CheckpointRecord, RunId, ThreadId};
use parking_lot::RwLock;

#[derive(Debug, Default, Clone)]
pub struct MemoryCheckpointStore {
    inner: Arc<RwLock<HashMap<(ThreadId, RunId), Vec<CheckpointRecord>>>>,
}

impl MemoryCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, record: CheckpointRecord) {
        let key = (record.thread_id, record.run_id);
        let mut g = self.inner.write();
        g.entry(key).or_default().push(record);
    }

    pub fn latest(&self, thread_id: ThreadId, run_id: RunId) -> Option<CheckpointRecord> {
        let g = self.inner.read();
        g.get(&(thread_id, run_id)).and_then(|v| v.last()).cloned()
    }

    pub fn clear_run(&self, thread_id: ThreadId, run_id: RunId) {
        let mut g = self.inner.write();
        g.remove(&(thread_id, run_id));
    }
}

/// Build a checkpoint record wrapping thread state.
#[must_use]
pub fn make_checkpoint(
    thread_id: ThreadId,
    run_id: RunId,
    state: agent_ports::ThreadState,
) -> CheckpointRecord {
    let step_seq = state.step_seq;
    CheckpointRecord {
        id: CheckpointId::new_v4(),
        thread_id,
        run_id,
        step_seq,
        state,
        metadata: serde_json::json!({}),
    }
}
