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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    pub thread_id: Uuid,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentTask {
    pub task_id: Uuid,
    pub thread_id: Uuid,
    pub agent_name: String,
    pub status: String,
    pub input: serde_json::Value,
    pub output: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExecution {
    pub execution_id: Uuid,
    pub thread_id: Uuid,
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub created_at: DateTime<Utc>,
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

#[async_trait]
pub trait SkillStore: Send + Sync {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError>;
    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError>;
    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError>;
}

#[async_trait]
pub trait ToolRecordStore: Send + Sync {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError>;
    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError>;
}

#[async_trait]
pub trait SubagentTaskStore: Send + Sync {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError>;
    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError>;
    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError>;
}

#[async_trait]
pub trait SandboxExecutionStore: Send + Sync {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError>;
    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError>;
}
