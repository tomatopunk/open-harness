//! Checkpoint records for resume/replay.

use serde::{Deserialize, Serialize};

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
}

fn default_checkpoint_record_schema() -> u32 {
    CHECKPOINT_RECORD_SCHEMA_VERSION
}
