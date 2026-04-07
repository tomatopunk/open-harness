//! Frozen schema versions for state and events (beta: bump on breaking changes).

/// `ThreadState` JSON evolution.
pub const THREAD_STATE_SCHEMA_VERSION: u32 = 6;

/// `AgentEvent` stream evolution (see [`crate::events::EventSink::event_schema_version`]).
pub const AGENT_EVENT_SCHEMA_VERSION: u32 = 3;

/// Serialized [`crate::checkpoint::CheckpointRecord`] envelope.
pub const CHECKPOINT_RECORD_SCHEMA_VERSION: u32 = 3;
