//! Pluggable storage abstraction for threads, checkpoints, artifacts, memory.

pub mod checkpoint_port;
pub mod delete_thread_report;
pub mod local_fs;
pub mod local_fs_store;
pub mod memory;
pub mod memory_atomic;
pub mod memory_document;
pub mod memory_extractor;
pub mod memory_injection;
pub mod memory_manager;
pub mod memory_merge;
pub mod memory_prompt;
pub mod memory_retrieval;
pub mod memory_system;
pub mod memory_voting;
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
pub use memory::config::MemoryConfig;
pub use memory::prompts::{MEMORY_UPDATE_PROMPT, MERGE_PROFILE_PROMPT};
pub use memory_atomic::AtomicMemoryStore;
pub use memory_document::{MemoryDocument, MEMORY_DOCUMENT_SCHEMA_VERSION};
pub use memory_extractor::{FactExtractor, MemoryUpdate, ProcessedUpdate};
pub use memory_injection::{
    format_complete_memory, inject_memory_to_prompt, MemoryInjectionConfig,
};
pub use memory_manager::{FactManager, FactStats};
pub use memory_merge::{MemoryMergeEngine, MergeResult, UserProfile};
pub use memory_prompt::{format_fact, format_facts, format_memory_for_prompt};
pub use memory_retrieval::{
    format_memory_for_injection, truncate_to_token_budget, SimpleTokenCounter, TokenCounter,
    TruncationResult,
};
pub use memory_system::{FactExtractionResult, MemorySystem, MemorySystemStats};
pub use memory_voting::{FactVote, MemoryVotingEngine, RankedFact, VoteResult};
pub use path_safety::sanitize_thread_id;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointStore, ManageAppConfig, ManageConfigStore, ManageTaskRecord,
    ManageTaskStore, McpConfigStore, MemoryStore, SandboxExecution, SandboxExecutionStore,
    SkillRecord, SkillStore, StateError, SubagentTask, SubagentTaskStore, ThreadLifecycleStore,
    ThreadMeta, ThreadMetaStore, ThreadUploadStore, ToolRecord, ToolRecordStore,
};
