//! Structured outcome for [`super::traits::ThreadLifecycleStore::delete_thread_cascade_report`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

/// Whether the backend can roll back the whole delete as one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteConsistencyLevel {
    /// SQLite / Postgres: single transaction for thread-scoped rows.
    StrongPerThread,
    /// Redis / S3 / local_fs: multi-step; partial progress possible.
    BestEffort,
}

/// Ordered domains for cascade delete (aligned with SQL transaction order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteThreadPhase {
    Checkpoints,
    Memory,
    Tools,
    Subagents,
    Sandbox,
    ManageTasks,
    Artifacts,
    Uploads,
    ThreadMeta,
}

/// Result status of a delete attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteThreadStatus {
    Complete,
    Partial { failed_at: DeleteThreadPhase, error: String },
    Failed { at: DeleteThreadPhase, error: String },
}

/// Machine-readable report for auditing and retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteThreadReport {
    pub operation_id: Uuid,
    pub thread_id: Uuid,
    pub status: DeleteThreadStatus,
    pub completed_phases: Vec<DeleteThreadPhase>,
    pub consistency: DeleteConsistencyLevel,
    /// Whether calling delete again may succeed (always true for idempotent cascade).
    pub retryable: bool,
}

impl fmt::Display for DeleteThreadReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

impl DeleteThreadReport {
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.status {
            DeleteThreadStatus::Complete => "complete".to_string(),
            DeleteThreadStatus::Partial { failed_at, error } => {
                format!("partial: failed_at={failed_at:?}, error={error}")
            }
            DeleteThreadStatus::Failed { at, error } => {
                format!("failed: at={at:?}, error={error}")
            }
        }
    }

    #[must_use]
    pub fn success(&self) -> bool {
        matches!(self.status, DeleteThreadStatus::Complete)
    }
}

/// Per-domain residual probe for [`super::traits::ThreadLifecycleStore::verify_thread_deletion`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteVerifyReport {
    pub thread_id: Uuid,
    /// Phase -> true if any durable data for that domain still exists.
    pub residual_by_phase: HashMap<DeleteThreadPhase, bool>,
}

impl DeleteVerifyReport {
    #[must_use]
    pub fn any_residual(&self) -> bool {
        self.residual_by_phase.values().any(|v| *v)
    }
}
