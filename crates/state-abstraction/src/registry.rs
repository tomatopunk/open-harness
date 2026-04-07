use std::collections::HashMap;
use std::sync::Arc;

use crate::memory_system::{MemorySystem, MemorySystemConfig};
use crate::traits::{
    ArtifactStore, CheckpointStore, ManageConfigStore, ManageTaskStore, McpConfigStore,
    MemoryStore, SandboxExecutionStore, SkillStore, StateError, SubagentTaskStore,
    ThreadLifecycleStore, ThreadMetaStore, ThreadUploadStore, ToolRecordStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    default_memory_backend: StorageBackendKind,
    memory_backends: HashMap<StorageBackendKind, Arc<dyn MemoryStore>>,
}

impl StorageRegistry {
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_memory_backend(
        default_memory_backend: StorageBackendKind,
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
        let mut memory_backends = HashMap::new();
        memory_backends.insert(default_memory_backend, memory.clone());

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
            default_memory_backend,
            memory_backends,
        }
    }

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
        Self::new_with_memory_backend(
            StorageBackendKind::LocalFs,
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
        )
    }

    pub fn register_memory_backend(
        &mut self,
        kind: StorageBackendKind,
        store: Arc<dyn MemoryStore>,
    ) {
        if kind == self.default_memory_backend {
            self.memory = store.clone();
        }
        self.memory_backends.insert(kind, store);
    }

    pub fn set_default_memory_backend(
        &mut self,
        kind: StorageBackendKind,
    ) -> Result<(), StateError> {
        let store = self.resolve_memory_store(kind)?;
        self.default_memory_backend = kind;
        self.memory = store;
        Ok(())
    }

    pub fn resolve_memory_store(
        &self,
        kind: StorageBackendKind,
    ) -> Result<Arc<dyn MemoryStore>, StateError> {
        self.memory_backends.get(&kind).cloned().ok_or_else(|| {
            StateError::Backend(format!("memory backend {:?} is not registered", kind))
        })
    }

    pub fn resolve_default_memory_store(&self) -> Result<Arc<dyn MemoryStore>, StateError> {
        self.resolve_memory_store(self.default_memory_backend)
    }

    pub fn build_memory_system(
        &self,
        config: MemorySystemConfig,
        kind: Option<StorageBackendKind>,
    ) -> Result<MemorySystem<Arc<dyn MemoryStore>>, StateError> {
        let store = match kind {
            Some(kind) => self.resolve_memory_store(kind)?,
            None => self.resolve_default_memory_store()?,
        };

        Ok(MemorySystem::new(config, store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delete_thread_report::{
        DeleteConsistencyLevel, DeleteThreadReport, DeleteThreadStatus, DeleteVerifyReport,
    };
    use crate::memory_document::{Fact, FactCategory, MemoryDocument};
    use crate::traits::{
        ManageAppConfig, ManageTaskRecord, MemoryPersistence, SandboxExecution, SkillRecord,
        SubagentTask, ThreadMeta, ToolRecord,
    };
    use agent_ports::{CheckpointRecord, RunId, StepSeq, ThreadId};
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct MockStore {
        memory_docs: Arc<tokio::sync::RwLock<HashMap<Uuid, MemoryDocument>>>,
    }

    #[async_trait]
    impl ThreadMetaStore for MockStore {
        async fn upsert_thread(&self, _meta: &ThreadMeta) -> Result<(), StateError> {
            Ok(())
        }

        async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
            Ok(ThreadMeta {
                thread_id,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                label: None,
            })
        }

        async fn delete_thread_meta(&self, _thread_id: Uuid) -> Result<(), StateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl CheckpointStore for MockStore {
        async fn save_checkpoint(&self, _record: &CheckpointRecord) -> Result<(), StateError> {
            Ok(())
        }

        async fn load_latest_checkpoint(
            &self,
            _thread_id: ThreadId,
            _run_id: RunId,
        ) -> Result<Option<CheckpointRecord>, StateError> {
            Ok(None)
        }

        async fn load_checkpoint_at_step(
            &self,
            _thread_id: ThreadId,
            _run_id: RunId,
            _step_seq: StepSeq,
        ) -> Result<Option<CheckpointRecord>, StateError> {
            Ok(None)
        }

        async fn list_checkpoint_steps_for_run(
            &self,
            _thread_id: ThreadId,
            _run_id: RunId,
        ) -> Result<Vec<StepSeq>, StateError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ArtifactStore for MockStore {
        async fn put_artifact(
            &self,
            _thread_id: Uuid,
            _name: &str,
            _bytes: &[u8],
        ) -> Result<String, StateError> {
            Ok(String::new())
        }

        async fn get_artifact(
            &self,
            _thread_id: Uuid,
            _name: &str,
        ) -> Result<Option<Vec<u8>>, StateError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl ThreadUploadStore for MockStore {
        async fn list_upload_filenames(&self, _thread_id: Uuid) -> Result<Vec<String>, StateError> {
            Ok(Vec::new())
        }

        async fn put_upload(
            &self,
            _thread_id: Uuid,
            _filename: &str,
            _bytes: &[u8],
        ) -> Result<(), StateError> {
            Ok(())
        }

        async fn get_upload(
            &self,
            _thread_id: Uuid,
            _filename: &str,
        ) -> Result<Option<Vec<u8>>, StateError> {
            Ok(None)
        }

        async fn delete_upload(&self, _thread_id: Uuid, _filename: &str) -> Result<(), StateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl MemoryPersistence for MockStore {
        async fn load_memory_document(
            &self,
            thread_id: Uuid,
        ) -> Result<MemoryDocument, StateError> {
            Ok(self.memory_docs.read().await.get(&thread_id).cloned().unwrap_or_default())
        }

        async fn save_memory_document(
            &self,
            thread_id: Uuid,
            doc: &MemoryDocument,
        ) -> Result<(), StateError> {
            self.memory_docs.write().await.insert(thread_id, doc.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl MemoryStore for MockStore {
        async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
            Ok(self.memory_docs.read().await.keys().cloned().collect())
        }
    }

    #[async_trait]
    impl SkillStore for MockStore {
        async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
            Ok(Vec::new())
        }

        async fn put_skill(&self, _record: &SkillRecord) -> Result<(), StateError> {
            Ok(())
        }

        async fn get_skill(&self, _name: &str) -> Result<Option<SkillRecord>, StateError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl ToolRecordStore for MockStore {
        async fn append_tool_record(&self, _record: &ToolRecord) -> Result<(), StateError> {
            Ok(())
        }

        async fn list_tool_records(&self, _thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl SubagentTaskStore for MockStore {
        async fn upsert_task(&self, _task: &SubagentTask) -> Result<(), StateError> {
            Ok(())
        }

        async fn get_task(&self, _task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
            Ok(None)
        }

        async fn list_tasks_by_thread(
            &self,
            _thread_id: Uuid,
        ) -> Result<Vec<SubagentTask>, StateError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl SandboxExecutionStore for MockStore {
        async fn append_execution(&self, _exec: &SandboxExecution) -> Result<(), StateError> {
            Ok(())
        }

        async fn list_executions(
            &self,
            _thread_id: Uuid,
        ) -> Result<Vec<SandboxExecution>, StateError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ManageTaskStore for MockStore {
        async fn upsert_task(&self, _task: &ManageTaskRecord) -> Result<(), StateError> {
            Ok(())
        }

        async fn get_task(&self, _task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
            Ok(None)
        }

        async fn list_tasks_by_thread(
            &self,
            _thread_id: &str,
        ) -> Result<Vec<ManageTaskRecord>, StateError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl McpConfigStore for MockStore {
        async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
            Ok(json!({}))
        }

        async fn put_mcp_servers(&self, _value: &serde_json::Value) -> Result<(), StateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ManageConfigStore for MockStore {
        async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
            Ok(ManageAppConfig::default())
        }

        async fn put_manage_app_config(&self, _cfg: &ManageAppConfig) -> Result<(), StateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ThreadLifecycleStore for MockStore {
        async fn delete_thread_cascade_report(
            &self,
            thread_id: Uuid,
        ) -> Result<DeleteThreadReport, StateError> {
            Ok(DeleteThreadReport {
                operation_id: Uuid::new_v4(),
                thread_id,
                status: DeleteThreadStatus::Complete,
                completed_phases: Vec::new(),
                consistency: DeleteConsistencyLevel::BestEffort,
                retryable: true,
            })
        }

        async fn last_delete_thread_report(
            &self,
            _thread_id: Uuid,
        ) -> Result<Option<DeleteThreadReport>, StateError> {
            Ok(None)
        }

        async fn verify_thread_deletion(
            &self,
            thread_id: Uuid,
        ) -> Result<DeleteVerifyReport, StateError> {
            Ok(DeleteVerifyReport { thread_id, residual_by_phase: HashMap::new() })
        }
    }

    fn registry_with_memory_backend(kind: StorageBackendKind) -> StorageRegistry {
        let store = Arc::new(MockStore::default());
        StorageRegistry::new_with_memory_backend(
            kind,
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store,
        )
    }

    #[tokio::test]
    async fn builds_memory_system_from_registered_backend() {
        let registry = registry_with_memory_backend(StorageBackendKind::LocalFs);
        let thread_id = Uuid::new_v4();
        let system = registry
            .build_memory_system(MemorySystemConfig::default(), None)
            .expect("default memory backend should resolve");

        system
            .add_fact(thread_id, "prefers dark mode".to_string(), FactCategory::Preference, 0.9)
            .await
            .expect("memory persistence should succeed");

        let loaded = system.load_memory(thread_id).await.expect("memory should load");
        assert_eq!(loaded.facts.len(), 1);
        assert_eq!(loaded.facts[0].content, "prefers dark mode");
    }

    #[test]
    fn resolve_memory_store_reports_missing_backend() {
        let registry = registry_with_memory_backend(StorageBackendKind::LocalFs);
        match registry.resolve_memory_store(StorageBackendKind::Sqlite) {
            Err(StateError::Backend(message)) => assert!(message.contains("Sqlite")),
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("unregistered backend should fail"),
        }
    }

    #[tokio::test]
    async fn register_memory_backend_updates_resolution_without_changing_persistence_api() {
        let mut registry = registry_with_memory_backend(StorageBackendKind::LocalFs);
        let sqlite_store = Arc::new(MockStore::default());
        let thread_id = Uuid::new_v4();

        registry.register_memory_backend(StorageBackendKind::Sqlite, sqlite_store);

        let system = registry
            .build_memory_system(MemorySystemConfig::default(), Some(StorageBackendKind::Sqlite))
            .expect("registered backend should resolve");

        let mut document = MemoryDocument::default();
        document.add_fact(Fact::new(
            "prefers keyboard shortcuts".to_string(),
            FactCategory::Preference,
            0.8,
            thread_id.to_string(),
        ));

        system.save_memory(thread_id, &document).await.expect("save should succeed");

        let loaded = system.load_memory(thread_id).await.expect("load should succeed");
        assert_eq!(loaded.facts.len(), 1);
        assert_eq!(loaded.facts[0].content, "prefers keyboard shortcuts");
    }
}
