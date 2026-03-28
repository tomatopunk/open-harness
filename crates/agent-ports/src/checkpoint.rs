//! Checkpoint records for resume/replay.

use serde::{Deserialize, Serialize};

use crate::ids::{CheckpointId, RunId, StepSeq, ThreadId};
use crate::thread_state::ThreadState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub id: CheckpointId,
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub step_seq: StepSeq,
    pub state: ThreadState,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
