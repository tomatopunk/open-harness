//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod checkpoint_port;
pub mod delete_thread_report;
pub mod local_fs;
pub mod local_fs_store;
pub mod memory_document;
pub mod path_safety;
pub mod registry;
pub mod traits;

pub use agent_ports::CheckpointRecord;
pub use checkpoint_port::DynCheckpointStorePort;
pub use delete_thread_report::{
    DeleteConsistencyLevel, DeleteThreadPhase, DeleteThreadReport, DeleteThreadStatus,
    DeleteVerifyReport,
};
pub use local_fs::LocalFsLayout;
pub use local_fs_store::LocalFsStateStore;
pub use memory_document::{MemoryDocument, MEMORY_DOCUMENT_SCHEMA_VERSION};
pub use path_safety::sanitize_thread_id;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointStore, ManageAppConfig, ManageConfigStore, ManageTaskRecord,
    ManageTaskStore, McpConfigStore, MemoryStore, SandboxExecution, SandboxExecutionStore,
    SkillRecord, SkillStore, StateError, SubagentTask, SubagentTaskStore, ThreadLifecycleStore,
    ThreadMeta, ThreadMetaStore, ThreadUploadStore, ToolRecord, ToolRecordStore,
};
