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
pub use memory_system::{FactExtractionResult, MemorySystem, MemorySystemError, MemorySystemStats};
pub use memory_voting::{FactVote, MemoryVotingEngine, RankedFact, VoteResult};
pub use path_safety::sanitize_thread_id;
pub use registry::{StorageBackendKind, StorageRegistry};
pub use traits::{
    ArtifactStore, CheckpointStore, ManageAppConfig, ManageConfigStore, ManageTaskRecord,
    ManageTaskStore, McpConfigStore, MemoryPersistence, MemoryStore, SandboxExecution,
    SandboxExecutionStore, SkillRecord, SkillStore, StateError, StateErrorCategory, SubagentTask,
    SubagentTaskStore, ThreadLifecycleStore, ThreadMeta, ThreadMetaStore, ThreadUploadStore,
    ToolRecord, ToolRecordStore, UnifiedConfigStore,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Storage mode enum
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    LocalFs,
    Sqlite,
    Postgres,
}

/// Local FS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalFsConfig {
    pub root: PathBuf,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(rename = "mode")]
    pub mode: StorageMode,
    pub local_fs: Option<LocalFsConfig>,
    pub sqlite: Option<()>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            mode: StorageMode::LocalFs,
            local_fs: Some(LocalFsConfig { root: PathBuf::from(".deer-flow/local-fs") }),
            sqlite: None,
        }
    }
}

/// Create a MemoryStore from configuration.
///
/// Factory method that creates the appropriate MemoryStore implementation based on storage mode.
/// Also handles directory creation for filesystem-based backends.
pub fn create_memory_store(
    storage_config: &StorageConfig,
    workspace_root: &std::path::Path,
    memory_storage_path: &str,
) -> Result<Box<dyn MemoryStore>, StateError> {
    match storage_config.mode {
        StorageMode::LocalFs => {
            // Build full storage path
            let default_root = PathBuf::from(".deer-flow/local-fs");
            let local_fs_root =
                storage_config.local_fs.as_ref().map(|l| &l.root).unwrap_or(&default_root);
            let full_path = workspace_root.join(local_fs_root).join(memory_storage_path);

            // Create parent directory if needed
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    StateError::Initialization(format!("Failed to create memory directory: {}", e))
                })?;
            }

            Ok(Box::new(LocalFsStateStore::new(full_path)))
        }
        // Other storage modes will be added as implementations become available
        _ => Err(StateError::Config(format!(
            "Storage mode {:?} not implemented for memory store",
            storage_config.mode
        ))),
    }
}
