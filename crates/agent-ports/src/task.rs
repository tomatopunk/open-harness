//! Task envelopes for superstep scheduling (LangGraph-style PUSH/PULL task surface).
//!
//! Staged tasks are persisted in [`crate::thread_state::PregelMeta::staged_tasks`] for replay
//! and inspection; execution semantics live in `agent-loop-runtime`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Static graph node triggered by channel updates (PULL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TaskKind {
    /// Named engine phase node (lead, premodel, postmodel, dispatch, ...).
    Pull { node_id: String },
    /// Dynamic fan-out: `call_id` is `ToolCallSpec::call_id` for `tool_invoke`, or `subagent:{idx}` for subagent slots.
    Push { fanout_id: String, call_id: String },
}

/// One schedulable unit within a superstep.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEnvelope {
    pub id: Uuid,
    pub kind: TaskKind,
}

impl TaskEnvelope {
    #[must_use]
    pub fn pull(node_id: impl Into<String>) -> Self {
        Self { id: Uuid::new_v4(), kind: TaskKind::Pull { node_id: node_id.into() } }
    }

    #[must_use]
    pub fn push_with_call_id(fanout_id: impl Into<String>, call_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: TaskKind::Push { fanout_id: fanout_id.into(), call_id: call_id.into() },
        }
    }
}
