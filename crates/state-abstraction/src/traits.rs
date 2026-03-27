use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("backend: {0}")]
    Backend(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMeta {
    pub thread_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointBlob {
    pub thread_id: Uuid,
    pub checkpoint_id: String,
    pub payload: Vec<u8>,
}

#[async_trait]
pub trait ThreadMetaStore: Send + Sync {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError>;
    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError>;
    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError>;
}

#[async_trait]
pub trait CheckpointStore: Send + Sync {
    async fn save_checkpoint(&self, blob: &CheckpointBlob) -> Result<(), StateError>;
    async fn load_checkpoint(&self, thread_id: Uuid) -> Result<Option<CheckpointBlob>, StateError>;
}

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError>;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append_fact(&self, thread_id: Uuid, fact: &str) -> Result<(), StateError>;
    async fn list_facts(&self, thread_id: Uuid) -> Result<Vec<String>, StateError>;
}
