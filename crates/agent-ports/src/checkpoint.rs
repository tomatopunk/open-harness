//! Checkpoint records for resume/replay.
//!
//! **LangGraph `put_writes` equivalence**: LangGraph may persist intermediate channel writes separately from the main checkpoint tuple.
//! In harness, pending / interrupt-related write context is carried in [`ThreadState::pregel`] and summarized in
//! [`crate::checkpoint_engine::EngineCheckpointExtensions`] (`pending_writes_count`, `resume_cursor`, etc.) on each
//! [`CheckpointRecord`]. Auditing “what was pending at this step” uses the same checkpoint row as the graph state — there is no
//! second hidden write log for the engine path.

use serde::{Deserialize, Serialize};

use crate::checkpoint_engine::EngineCheckpointExtensions;
use crate::ids::{CheckpointId, RunId, StepSeq, ThreadId};
use crate::schema::CHECKPOINT_RECORD_SCHEMA_VERSION;
use crate::thread_state::ThreadState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    #[serde(default = "default_checkpoint_record_schema")]
    pub record_schema_version: u32,
    pub id: CheckpointId,
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub step_seq: StepSeq,
    pub state: ThreadState,
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// Engine kernel metadata (resume cursor, schema pinning). Distinct from opaque `metadata`.
    #[serde(default)]
    pub engine: EngineCheckpointExtensions,
}

fn default_checkpoint_record_schema() -> u32 {
    CHECKPOINT_RECORD_SCHEMA_VERSION
}
