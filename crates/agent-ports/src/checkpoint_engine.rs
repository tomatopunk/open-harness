//! Engine-specific checkpoint extensions (durable execution metadata; distinct from host `metadata` JSON).

use serde::{Deserialize, Serialize};

use crate::state_effect::StateEffect;

/// Extra fields persisted alongside [`crate::checkpoint::CheckpointRecord`] for resume / migration hooks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineCheckpointExtensions {
    /// Snapshot of [`crate::thread_state::ThreadState::state_schema_version`] at commit time.
    #[serde(default)]
    pub state_schema_version: u32,
    /// Count of logical pending writes bundled into this checkpoint (for future multi-write semantics).
    #[serde(default)]
    pub pending_writes_count: u32,
    /// Opaque resume cursor for host-driven interrupt/resume (e.g. clarify token).
    #[serde(default)]
    pub resume_cursor: Option<String>,
    /// State effects not yet merged into `CheckpointRecord::state` (crash recovery; applied on resume).
    #[serde(default)]
    pub pending_state_effects: Vec<StateEffect>,
    /// Last completed superstep (mirrors [`crate::thread_state::PregelMeta::superstep_seq`] when committed).
    #[serde(default)]
    pub superstep_seq: u64,
}
