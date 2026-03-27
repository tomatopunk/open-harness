use std::sync::Arc;

use crate::traits::{ArtifactStore, CheckpointStore, MemoryStore, ThreadMetaStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackendKind {
    Sqlite,
    Postgres,
    Redis,
    S3,
}

/// Holds concrete store implementations selected by configuration.
pub struct StorageRegistry {
    pub threads: Arc<dyn ThreadMetaStore>,
    pub checkpoints: Arc<dyn CheckpointStore>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub memory: Arc<dyn MemoryStore>,
}

impl StorageRegistry {
    pub fn new(
        threads: Arc<dyn ThreadMetaStore>,
        checkpoints: Arc<dyn CheckpointStore>,
        artifacts: Arc<dyn ArtifactStore>,
        memory: Arc<dyn MemoryStore>,
    ) -> Self {
        Self { threads, checkpoints, artifacts, memory }
    }
}
