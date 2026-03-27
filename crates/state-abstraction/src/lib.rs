//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod local_fs;
pub mod registry;
pub mod traits;

pub use local_fs::LocalFsLayout;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointBlob, CheckpointStore, MemoryStore, StateError, ThreadMeta,
    ThreadMetaStore,
};
