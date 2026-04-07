//! First-class interrupt / resume (LangGraph-style `interrupt` + `Command(resume)` semantics).

use crate::ids::StepSeq;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cursor persisted with an interrupt so execution can resume after reload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResumeCursor {
    /// Engine phase node where execution paused (e.g. `dispatch_clarify`).
    pub node_id: String,
    /// [`crate::thread_state::PregelMeta::superstep_seq`] at interrupt time.
    pub superstep_seq: u64,
    /// Monotonic step within the run (mirrors checkpoint cadence).
    pub step_seq: StepSeq,
}

/// Why execution paused (extensible).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InterruptKind {
    /// Model requested user clarification (legacy Clarify path unified here).
    Clarification { prompt: Option<String> },
}

/// Durable interrupt snapshot stored in [`crate::thread_state::PregelMeta::interrupt`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptSnapshot {
    pub kind: InterruptKind,
    pub resume_cursor: ResumeCursor,
    /// Opaque payload for hosts (e.g. model hints); keep JSON for version tolerance.
    #[serde(default)]
    pub payload: Value,
}
