//! Bridges [`crate::traits::CheckpointStore`] to [`agent_ports::CheckpointPort`].

use std::sync::Arc;

use agent_ports::{
    CheckpointPort, CheckpointRecord, PortError, PortResult, RunId, StepSeq, ThreadId,
};
use async_trait::async_trait;

use crate::traits::{CheckpointStore, StateError};

#[derive(Clone)]
pub struct DynCheckpointStorePort {
    inner: Arc<dyn CheckpointStore>,
}

impl DynCheckpointStorePort {
    #[must_use]
    pub fn new(inner: Arc<dyn CheckpointStore>) -> Self {
        Self { inner }
    }
}

fn map_err(e: StateError) -> PortError {
    PortError::Checkpoint(e.to_string())
}

#[async_trait]
impl CheckpointPort for DynCheckpointStorePort {
    async fn save(&self, record: CheckpointRecord) -> PortResult<()> {
        self.inner.save_checkpoint(&record).await.map_err(map_err)
    }

    async fn load_latest(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> PortResult<Option<CheckpointRecord>> {
        self.inner.load_latest_checkpoint(thread_id, run_id).await.map_err(map_err)
    }

    async fn load_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> PortResult<Option<CheckpointRecord>> {
        self.inner.load_checkpoint_at_step(thread_id, run_id, step_seq).await.map_err(map_err)
    }

    async fn list_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> PortResult<Vec<StepSeq>> {
        self.inner.list_checkpoint_steps_for_run(thread_id, run_id).await.map_err(map_err)
    }
}
