use std::sync::Arc;

use crate::traits::{
    ArtifactStore, CheckpointStore, ManageConfigStore, ManageTaskStore, McpConfigStore,
    MemoryStore, SandboxExecutionStore, SkillStore, SubagentTaskStore, ThreadLifecycleStore,
    ThreadMetaStore, ThreadUploadStore, ToolRecordStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackendKind {
    LocalFs,
    Sqlite,
    Postgres,
    Redis,
    S3,
}

impl StorageBackendKind {
    pub fn from_mode(mode: &str) -> Self {
        match mode {
            "local_fs" => Self::LocalFs,
            "sqlite" => Self::Sqlite,
            "postgres" => Self::Postgres,
            "redis" => Self::Redis,
            "s3" => Self::S3,
            _ => Self::LocalFs,
        }
    }
}

/// Holds concrete store implementations selected by configuration.
#[derive(Clone)]
pub struct StorageRegistry {
    pub threads: Arc<dyn ThreadMetaStore>,
    pub checkpoints: Arc<dyn CheckpointStore>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub uploads: Arc<dyn ThreadUploadStore>,
    pub memory: Arc<dyn MemoryStore>,
    pub skills: Arc<dyn SkillStore>,
    pub tools: Arc<dyn ToolRecordStore>,
    pub subagents: Arc<dyn SubagentTaskStore>,
    pub sandbox: Arc<dyn SandboxExecutionStore>,
    pub manage_tasks: Arc<dyn ManageTaskStore>,
    pub mcp_config: Arc<dyn McpConfigStore>,
    pub manage_config: Arc<dyn ManageConfigStore>,
    pub lifecycle: Arc<dyn ThreadLifecycleStore>,
}

impl StorageRegistry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        threads: Arc<dyn ThreadMetaStore>,
        checkpoints: Arc<dyn CheckpointStore>,
        artifacts: Arc<dyn ArtifactStore>,
        uploads: Arc<dyn ThreadUploadStore>,
        memory: Arc<dyn MemoryStore>,
        skills: Arc<dyn SkillStore>,
        tools: Arc<dyn ToolRecordStore>,
        subagents: Arc<dyn SubagentTaskStore>,
        sandbox: Arc<dyn SandboxExecutionStore>,
        manage_tasks: Arc<dyn ManageTaskStore>,
        mcp_config: Arc<dyn McpConfigStore>,
        manage_config: Arc<dyn ManageConfigStore>,
        lifecycle: Arc<dyn ThreadLifecycleStore>,
    ) -> Self {
        Self {
            threads,
            checkpoints,
            artifacts,
            uploads,
            memory,
            skills,
            tools,
            subagents,
            sandbox,
            manage_tasks,
            mcp_config,
            manage_config,
            lifecycle,
        }
    }
}
