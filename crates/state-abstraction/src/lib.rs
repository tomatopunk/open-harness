//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod local_fs;
pub mod local_fs_store;
pub mod registry;
pub mod traits;

pub use local_fs::LocalFsLayout;
pub use local_fs_store::LocalFsStateStore;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointBlob, CheckpointStore, MemoryStore, SandboxExecution,
    SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask, SubagentTaskStore,
    ThreadMeta, ThreadMetaStore, ToolRecord, ToolRecordStore,
};
