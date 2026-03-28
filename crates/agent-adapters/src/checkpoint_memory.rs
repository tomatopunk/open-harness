//! In-memory checkpoint port.

use std::collections::HashMap;
use std::sync::Arc;

use agent_ports::{CheckpointPort, CheckpointRecord, RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use parking_lot::RwLock;

#[derive(Debug, Default, Clone)]
pub struct MemoryCheckpointAdapter {
    inner: Arc<RwLock<HashMap<(ThreadId, RunId), Vec<CheckpointRecord>>>>,
}

#[async_trait]
impl CheckpointPort for MemoryCheckpointAdapter {
    async fn save(&self, record: CheckpointRecord) -> agent_ports::PortResult<()> {
        let key = (record.thread_id, record.run_id);
        self.inner.write().entry(key).or_default().push(record);
        Ok(())
    }

    async fn load_latest(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> agent_ports::PortResult<Option<CheckpointRecord>> {
        let g = self.inner.read();
        Ok(g.get(&(thread_id, run_id)).and_then(|v| v.last()).cloned())
    }

    async fn load_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> agent_ports::PortResult<Option<CheckpointRecord>> {
        let g = self.inner.read();
        Ok(g.get(&(thread_id, run_id))
            .and_then(|v| v.iter().find(|r| r.step_seq == step_seq))
            .cloned())
    }
}
