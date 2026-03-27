//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod local_fs;
pub mod local_fs_store;
pub mod path_safety;
pub mod registry;
pub mod traits;

pub use local_fs::LocalFsLayout;
pub use local_fs_store::LocalFsStateStore;
pub use path_safety::sanitize_thread_id;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointBlob, CheckpointStore, ManageTaskRecord, ManageTaskStore, MemoryStore,
    SandboxExecution, SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask,
    SubagentTaskStore, ThreadMeta, ThreadMetaStore, ToolRecord, ToolRecordStore,
};
