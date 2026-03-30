use agent_ports::{CheckpointRecord, RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Cascade delete did not reach [`crate::delete_thread_report::DeleteThreadStatus::Complete`].
    #[error("{0}")]
    LifecycleIncomplete(Box<crate::delete_thread_report::DeleteThreadReport>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadMeta {
    pub thread_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub label: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManageTaskRecord {
    pub task_id: String,
    pub thread_id: String,
    pub status: String,
    pub output_chunks: Vec<String>,
    pub error: Option<String>,
    pub callback_url: Option<String>,
    pub stream: bool,
    pub client_task_id: Option<String>,
    pub tenant_id: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[async_trait]
pub trait ThreadMetaStore: Send + Sync {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError>;
    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError>;
    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError>;
}

#[async_trait]
pub trait CheckpointStore: Send + Sync {
    /// Persist one checkpoint record (append per step under thread/run).
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError>;
    async fn load_latest_checkpoint(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Option<CheckpointRecord>, StateError>;
    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError>;
    /// All step indices that have a checkpoint for this thread/run (sorted ascending). Used for time-travel / debug listing.
    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError>;
}

#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError>;

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError>;
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    /// Full structured memory (see [`crate::memory_document::MemoryDocument`]). Missing thread → empty document.
    async fn load_memory_document(
        &self,
        thread_id: Uuid,
    ) -> Result<crate::memory_document::MemoryDocument, StateError>;
    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &crate::memory_document::MemoryDocument,
    ) -> Result<(), StateError>;

    async fn append_fact(&self, thread_id: Uuid, fact: &str) -> Result<(), StateError> {
        let mut doc = self.load_memory_document(thread_id).await?;
        // Create a default fact with the provided content
        let new_fact = crate::memory_document::Fact::new(
            fact.to_string(),
            crate::memory_document::FactCategory::default(),
            0.5, // Default confidence
            thread_id.to_string(),
        );
        doc.add_fact(new_fact);
        self.save_memory_document(thread_id, &doc).await
    }

    async fn list_facts(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let doc = self.load_memory_document(thread_id).await?;
        // Return fact contents as strings for backward compatibility
        Ok(doc.facts.iter().map(|f| f.content.clone()).collect())
    }

    /// Threads that have durable memory content (facts or structured user/history JSON).
    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError>;
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

#[async_trait]
pub trait ManageTaskStore: Send + Sync {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError>;
    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError>;
    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError>;
}

/// MCP server configuration persisted with other runtime state (not thread-scoped).
#[async_trait]
pub trait McpConfigStore: Send + Sync {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError>;
    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError>;
}

/// Manage UI / routing state (agents, channels, model list metadata) — not MCP servers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManageAppConfig {
    /// Agent id -> definition JSON.
    pub agents: HashMap<String, serde_json::Value>,
    pub channels: HashMap<String, String>,
    pub models: Vec<serde_json::Value>,
}

#[async_trait]
pub trait ManageConfigStore: Send + Sync {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError>;
    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError>;
}

/// Thread-scoped user uploads (separate from [`ArtifactStore`] engine artifacts).
#[async_trait]
pub trait ThreadUploadStore: Send + Sync {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError>;
    async fn put_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), StateError>;
    async fn get_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, StateError>;
    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError>;
}

/// Deletes all durable state for a thread (checkpoints, memory, tools, uploads, …).
#[async_trait]
pub trait ThreadLifecycleStore: Send + Sync {
    /// Structured outcome; persists an audit record where the backend supports it.
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<crate::delete_thread_report::DeleteThreadReport, StateError>;

    /// Idempotent: missing data is not an error for individual domains; returns `Err(LifecycleIncomplete)` unless fully complete.
    async fn delete_thread_cascade(&self, thread_id: Uuid) -> Result<(), StateError> {
        let r = self.delete_thread_cascade_report(thread_id).await?;
        if matches!(r.status, crate::delete_thread_report::DeleteThreadStatus::Complete) {
            Ok(())
        } else {
            Err(StateError::LifecycleIncomplete(Box::new(r)))
        }
    }

    /// Last persisted delete report for this thread, if any.
    async fn last_delete_thread_report(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<crate::delete_thread_report::DeleteThreadReport>, StateError>;

    /// Best-effort residual check per domain (may be expensive on object stores).
    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<crate::delete_thread_report::DeleteVerifyReport, StateError>;
}

/// Unified configuration store for runtime state.
#[async_trait]
pub trait UnifiedConfigStore: Send + Sync {
    /// Get the unified configuration.
    async fn get_unified_config(&self) -> Result<unified_config::UnifiedConfig, StateError>;

    /// Put the unified configuration.
    async fn put_unified_config(
        &self,
        config: &unified_config::UnifiedConfig,
    ) -> Result<(), StateError>;
}
