//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod registry;
pub mod traits;

pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointBlob, CheckpointStore, MemoryStore, StateError, ThreadMeta,
    ThreadMetaStore,
};
