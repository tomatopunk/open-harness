//! Engine-specific checkpoint extensions (durable execution metadata; distinct from host `metadata` JSON).

use serde::{Deserialize, Serialize};

/// Extra fields persisted alongside [`crate::checkpoint::CheckpointRecord`] for resume / migration hooks.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
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
}
