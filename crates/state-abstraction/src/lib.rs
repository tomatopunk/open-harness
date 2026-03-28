//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod checkpoint_port;
pub mod local_fs;
pub mod local_fs_store;
pub mod path_safety;
pub mod registry;
pub mod traits;

pub use agent_ports::CheckpointRecord;
pub use checkpoint_port::DynCheckpointStorePort;
pub use local_fs::LocalFsLayout;
pub use local_fs_store::LocalFsStateStore;
pub use path_safety::sanitize_thread_id;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointStore, ManageTaskRecord, ManageTaskStore, MemoryStore,
    SandboxExecution, SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask,
    SubagentTaskStore, ThreadMeta, ThreadMetaStore, ToolRecord, ToolRecordStore,
};
