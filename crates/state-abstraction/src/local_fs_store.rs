use agent_ports::{CheckpointRecord, RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use chrono::Utc;
use std::io::ErrorKind;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

use crate::delete_thread_report::{
    DeleteConsistencyLevel, DeleteThreadPhase, DeleteThreadReport, DeleteThreadStatus,
    DeleteVerifyReport,
};
use crate::path_safety::sanitize_thread_id;
use crate::traits::{
    ArtifactStore, CheckpointStore, ManageAppConfig, ManageConfigStore, ManageTaskRecord,
    ManageTaskStore, McpConfigStore, MemoryPersistence, MemoryStore, SandboxExecution,
    SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask, SubagentTaskStore,
    ThreadLifecycleStore, ThreadMeta, ThreadMetaStore, ThreadUploadStore, ToolRecord,
    ToolRecordStore, UnifiedConfigStore,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LocalFsStateStore {
    root: PathBuf,
}

impl LocalFsStateStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn thread_meta_path(&self) -> PathBuf {
        self.root.join("state").join("thread_meta.json")
    }

    fn checkpoint_run_dir(&self, thread_id: ThreadId, run_id: RunId) -> PathBuf {
        self.root
            .join("state")
            .join("checkpoints")
            .join(thread_id.0.to_string())
            .join(run_id.0.to_string())
    }

    fn checkpoint_steps_dir(&self, thread_id: ThreadId, run_id: RunId) -> PathBuf {
        self.checkpoint_run_dir(thread_id, run_id).join("steps")
    }

    fn checkpoint_step_path(&self, thread_id: ThreadId, run_id: RunId, step: StepSeq) -> PathBuf {
        self.checkpoint_steps_dir(thread_id, run_id).join(format!("{:020}.json", step.0))
    }

    fn checkpoint_latest_path(&self, thread_id: ThreadId, run_id: RunId) -> PathBuf {
        self.checkpoint_run_dir(thread_id, run_id).join("latest.json")
    }

    fn memory_path(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("memory").join(format!("{thread_id}.json"))
    }

    fn skills_path(&self) -> PathBuf {
        self.root.join("config").join("skills.json")
    }

    fn tools_path(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("tasks").join(format!("tools-{thread_id}.json"))
    }

    fn subagent_path(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("tasks").join(format!("subagents-{thread_id}.json"))
    }

    fn sandbox_path(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("tasks").join(format!("sandbox-{thread_id}.json"))
    }

    fn manage_task_path(&self, thread_id: &str) -> PathBuf {
        self.root.join("tasks").join(format!("manage-{thread_id}.json"))
    }

    fn require_valid_thread_id(thread_id: &str) -> Result<(), StateError> {
        if sanitize_thread_id(thread_id).is_some() {
            Ok(())
        } else {
            Err(StateError::Backend(format!("invalid thread_id: {}", thread_id)))
        }
    }

    fn artifact_path(&self, thread_id: Uuid, name: &str) -> PathBuf {
        self.root.join("artifacts").join(thread_id.to_string()).join(name)
    }

    fn uploads_dir(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("uploads").join(thread_id.to_string())
    }

    fn upload_path(&self, thread_id: Uuid, filename: &str) -> PathBuf {
        self.uploads_dir(thread_id).join(filename)
    }

    fn manage_app_config_path(&self) -> PathBuf {
        self.root.join("config").join("manage_app.json")
    }

    fn unified_config_path(&self) -> PathBuf {
        self.root.join("config").join("unified_config.json")
    }

    fn thread_ops_last_path(&self, thread_id: Uuid) -> PathBuf {
        self.root.join("state").join("thread_ops").join(thread_id.to_string()).join("last.json")
    }

    async fn persist_lifecycle_audit_local_fs(
        &self,
        report: &DeleteThreadReport,
    ) -> Result<(), StateError> {
        let path = self.thread_ops_last_path(report.thread_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StateError::Backend(format!("mkdir thread_ops: {e}")))?;
        }
        let bytes = serde_json::to_vec_pretty(report)
            .map_err(|e| StateError::Backend(format!("audit json: {e}")))?;
        fs::write(&path, bytes)
            .await
            .map_err(|e| StateError::Backend(format!("write audit: {e}")))?;
        Ok(())
    }

    async fn delete_thread_meta_if_present(&self, thread_id: Uuid) -> Result<(), StateError> {
        let path = self.thread_meta_path();
        let mut items = self.read_json_vec::<ThreadMeta>(&path).await?;
        let before = items.len();
        items.retain(|m| m.thread_id != thread_id);
        if items.len() == before {
            return Ok(());
        }
        self.write_json(&path, &items).await
    }

    async fn read_json_vec<T: for<'de> serde::Deserialize<'de>>(
        &self,
        path: &PathBuf,
    ) -> Result<Vec<T>, StateError> {
        match fs::read(path).await {
            Ok(bytes) => serde_json::from_slice::<Vec<T>>(&bytes)
                .map_err(|e| StateError::Backend(format!("json decode {}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(StateError::Backend(format!("read {}: {e}", path.display()))),
        }
    }

    async fn write_json<T: serde::Serialize>(
        &self,
        path: &PathBuf,
        value: &T,
    ) -> Result<(), StateError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StateError::Backend(format!("mkdir {}: {e}", parent.display())))?;
        }
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|e| StateError::Backend(format!("json encode {}: {e}", path.display())))?;
        fs::write(path, bytes)
            .await
            .map_err(|e| StateError::Backend(format!("write {}: {e}", path.display())))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadMetaStore for LocalFsStateStore {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError> {
        let path = self.thread_meta_path();
        let mut items = self.read_json_vec::<ThreadMeta>(&path).await?;
        if let Some(existing) = items.iter_mut().find(|m| m.thread_id == meta.thread_id) {
            *existing = meta.clone();
        } else {
            items.push(meta.clone());
        }
        self.write_json(&path, &items).await
    }

    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
        let path = self.thread_meta_path();
        let items = self.read_json_vec::<ThreadMeta>(&path).await?;
        items
            .into_iter()
            .find(|m| m.thread_id == thread_id)
            .ok_or_else(|| StateError::NotFound(thread_id.to_string()))
    }

    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError> {
        let path = self.thread_meta_path();
        let mut items = self.read_json_vec::<ThreadMeta>(&path).await?;
        let before = items.len();
        items.retain(|m| m.thread_id != thread_id);
        if items.len() == before {
            return Err(StateError::NotFound(thread_id.to_string()));
        }
        self.write_json(&path, &items).await
    }
}

#[async_trait]
impl CheckpointStore for LocalFsStateStore {
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError> {
        let step_path = self.checkpoint_step_path(record.thread_id, record.run_id, record.step_seq);
        let latest_path = self.checkpoint_latest_path(record.thread_id, record.run_id);
        self.write_json(&step_path, record).await?;
        self.write_json(&latest_path, record).await
    }

    async fn load_latest_checkpoint(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let path = self.checkpoint_latest_path(thread_id, run_id);
        match fs::read(path.clone()).await {
            Ok(bytes) => serde_json::from_slice::<CheckpointRecord>(&bytes)
                .map(Some)
                .map_err(|e| StateError::Backend(format!("json decode {}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Backend(format!("read {}: {e}", path.display()))),
        }
    }

    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let path = self.checkpoint_step_path(thread_id, run_id, step_seq);
        match fs::read(path.clone()).await {
            Ok(bytes) => serde_json::from_slice::<CheckpointRecord>(&bytes)
                .map(Some)
                .map_err(|e| StateError::Backend(format!("json decode {}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Backend(format!("read {}: {e}", path.display()))),
        }
    }

    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError> {
        let dir = self.checkpoint_steps_dir(thread_id, run_id);
        let mut rd = match fs::read_dir(&dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StateError::Backend(format!("read checkpoint steps dir: {e}"))),
        };
        let mut steps = Vec::new();
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name();
            let s = name.to_string_lossy();
            let Some(num) = s.strip_suffix(".json") else {
                continue;
            };
            let Ok(v) = num.parse::<u64>() else {
                continue;
            };
            steps.push(StepSeq(v));
        }
        steps.sort_by_key(|s| s.0);
        Ok(steps)
    }
}

#[async_trait]
impl ArtifactStore for LocalFsStateStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        let path = self.artifact_path(thread_id, name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StateError::Backend(format!("mkdir {}: {e}", parent.display())))?;
        }
        fs::write(path.clone(), bytes)
            .await
            .map_err(|e| StateError::Backend(format!("write {}: {e}", path.display())))?;
        Ok(path.to_string_lossy().to_string())
    }

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let path = self.artifact_path(thread_id, name);
        match fs::read(&path).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Backend(format!("read {}: {e}", path.display()))),
        }
    }
}

#[async_trait]
impl MemoryPersistence for LocalFsStateStore {
    async fn load_memory_document(
        &self,
        thread_id: Uuid,
    ) -> Result<crate::memory_document::MemoryDocument, StateError> {
        let path = self.memory_path(thread_id);
        match fs::read(&path).await {
            Ok(bytes) => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| StateError::Backend(format!("memory utf8: {e}")))?;
                crate::memory_document::decode_memory_json_str(&s).map_err(StateError::Backend)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Ok(crate::memory_document::MemoryDocument::default())
            }
            Err(e) => Err(StateError::Backend(format!("read memory: {e}"))),
        }
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &crate::memory_document::MemoryDocument,
    ) -> Result<(), StateError> {
        let path = self.memory_path(thread_id);
        if !doc.has_any_content() {
            match fs::remove_file(&path).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("remove memory: {e}"))),
            }
        } else {
            self.write_json(&path, doc).await
        }
    }
}

#[async_trait]
impl MemoryStore for LocalFsStateStore {
    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        let dir = self.root.join("memory");
        let mut rd = match fs::read_dir(dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StateError::Backend(format!("read memory dir: {e}"))),
        };
        let mut out = Vec::new();
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name();
            let Some(s) = name.to_str() else {
                continue;
            };
            let Some(id) = s.strip_suffix(".json") else {
                continue;
            };
            let Ok(u) = Uuid::parse_str(id) else {
                continue;
            };
            if self.load_memory_document(u).await?.has_any_content() {
                out.push(u);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl SkillStore for LocalFsStateStore {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
        self.read_json_vec::<SkillRecord>(&self.skills_path()).await
    }

    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError> {
        let path = self.skills_path();
        let mut items = self.read_json_vec::<SkillRecord>(&path).await?;
        if let Some(existing) = items.iter_mut().find(|s| s.name == record.name) {
            *existing = record.clone();
        } else {
            items.push(record.clone());
        }
        self.write_json(&path, &items).await
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError> {
        let items = self.list_skills().await?;
        Ok(items.into_iter().find(|s| s.name == name))
    }
}

#[async_trait]
impl ToolRecordStore for LocalFsStateStore {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError> {
        let path = self.tools_path(record.thread_id);
        let mut items = self.read_json_vec::<ToolRecord>(&path).await?;
        items.push(record.clone());
        self.write_json(&path, &items).await
    }

    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
        self.read_json_vec::<ToolRecord>(&self.tools_path(thread_id)).await
    }
}

#[async_trait]
impl SubagentTaskStore for LocalFsStateStore {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError> {
        let path = self.subagent_path(task.thread_id);
        let mut items = self.read_json_vec::<SubagentTask>(&path).await?;
        if let Some(existing) = items.iter_mut().find(|t| t.task_id == task.task_id) {
            *existing = task.clone();
        } else {
            items.push(task.clone());
        }
        self.write_json(&path, &items).await
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
        let dir = self.root.join("tasks");
        let mut rd = match fs::read_dir(dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StateError::Backend(format!("read tasks dir: {e}"))),
        };
        while let Ok(Some(ent)) = rd.next_entry().await {
            let path = ent.path();
            if !path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("subagents-"))
                .unwrap_or(false)
            {
                continue;
            }
            let items = self.read_json_vec::<SubagentTask>(&path).await?;
            if let Some(item) = items.into_iter().find(|i| i.task_id == task_id) {
                return Ok(Some(item));
            }
        }
        Ok(None)
    }

    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError> {
        self.read_json_vec::<SubagentTask>(&self.subagent_path(thread_id)).await
    }
}

#[async_trait]
impl SandboxExecutionStore for LocalFsStateStore {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError> {
        let path = self.sandbox_path(exec.thread_id);
        let mut items = self.read_json_vec::<SandboxExecution>(&path).await?;
        items.push(exec.clone());
        self.write_json(&path, &items).await
    }

    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError> {
        self.read_json_vec::<SandboxExecution>(&self.sandbox_path(thread_id)).await
    }
}

#[async_trait]
impl ManageTaskStore for LocalFsStateStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        Self::require_valid_thread_id(&task.thread_id)?;
        let path = self.manage_task_path(&task.thread_id);
        let mut items = self.read_json_vec::<ManageTaskRecord>(&path).await?;
        if let Some(existing) = items.iter_mut().find(|t| t.task_id == task.task_id) {
            *existing = task.clone();
        } else {
            items.push(task.clone());
        }
        self.write_json(&path, &items).await
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let dir = self.root.join("tasks");
        let mut rd = match fs::read_dir(dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StateError::Backend(format!("read tasks dir: {e}"))),
        };
        while let Ok(Some(ent)) = rd.next_entry().await {
            let path = ent.path();
            if !path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("manage-"))
                .unwrap_or(false)
            {
                continue;
            }
            let items = self.read_json_vec::<ManageTaskRecord>(&path).await?;
            if let Some(item) = items.into_iter().find(|i| i.task_id == task_id) {
                return Ok(Some(item));
            }
        }
        Ok(None)
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        Self::require_valid_thread_id(thread_id)?;
        self.read_json_vec::<ManageTaskRecord>(&self.manage_task_path(thread_id)).await
    }
}

#[async_trait]
impl McpConfigStore for LocalFsStateStore {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
        let path = self.root.join("config").join("mcp_servers.json");
        match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| StateError::Backend(format!("mcp json decode: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
            Err(e) => Err(StateError::Backend(format!("read mcp config: {e}"))),
        }
    }

    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError> {
        let path = self.root.join("config").join("mcp_servers.json");
        self.write_json(&path, value).await
    }
}

#[async_trait]
impl ThreadUploadStore for LocalFsStateStore {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let dir = self.uploads_dir(thread_id);
        let mut rd = match fs::read_dir(&dir).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StateError::Backend(format!("read uploads dir: {e}"))),
        };
        let mut out = Vec::new();
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Ok(ft) = ent.file_type().await {
                if ft.is_file() {
                    if let Some(name) = ent.file_name().to_str() {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    async fn put_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), StateError> {
        let path = self.upload_path(thread_id, filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StateError::Backend(format!("mkdir {}: {e}", parent.display())))?;
        }
        fs::write(&path, bytes)
            .await
            .map_err(|e| StateError::Backend(format!("write upload: {e}")))?;
        Ok(())
    }

    async fn get_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let path = self.upload_path(thread_id, filename);
        match fs::read(&path).await {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Backend(format!("read upload: {e}"))),
        }
    }

    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError> {
        let path = self.upload_path(thread_id, filename);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StateError::Backend(format!("remove upload: {e}"))),
        }
    }
}

#[async_trait]
impl ManageConfigStore for LocalFsStateStore {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
        let path = self.manage_app_config_path();
        match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| StateError::Backend(format!("manage_app json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ManageAppConfig::default()),
            Err(e) => Err(StateError::Backend(format!("read manage_app: {e}"))),
        }
    }

    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError> {
        self.write_json(&self.manage_app_config_path(), cfg).await
    }
}

#[async_trait]
impl UnifiedConfigStore for LocalFsStateStore {
    async fn get_unified_config(&self) -> Result<unified_config::UnifiedConfig, StateError> {
        let path = self.unified_config_path();
        match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| StateError::Backend(format!("unified_config json: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(unified_config::UnifiedConfig::default())
            }
            Err(e) => Err(StateError::Backend(format!("read unified_config: {e}"))),
        }
    }

    async fn put_unified_config(
        &self,
        config: &unified_config::UnifiedConfig,
    ) -> Result<(), StateError> {
        self.write_json(&self.unified_config_path(), config).await
    }
}

#[async_trait]
impl ThreadLifecycleStore for LocalFsStateStore {
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteThreadReport, StateError> {
        let operation_id = Uuid::new_v4();
        let tid = thread_id;
        let consistency = DeleteConsistencyLevel::BestEffort;
        let mut completed = Vec::new();

        macro_rules! phase {
            ($p:expr, $f:expr) => {
                if let Err(e) = $f.await {
                    let r = DeleteThreadReport {
                        operation_id,
                        thread_id: tid,
                        status: DeleteThreadStatus::Partial { failed_at: $p, error: e.to_string() },
                        completed_phases: completed.clone(),
                        consistency,
                        retryable: true,
                    };
                    let _ = self.persist_lifecycle_audit_local_fs(&r).await;
                    return Ok(r);
                }
                completed.push($p);
            };
        }

        let cp_root = self.root.join("state").join("checkpoints").join(tid.to_string());
        phase!(DeleteThreadPhase::Checkpoints, async {
            match fs::remove_dir_all(&cp_root).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("checkpoints: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::Memory, async {
            match fs::remove_file(self.memory_path(tid)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("memory: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::Tools, async {
            match fs::remove_file(self.tools_path(tid)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("tools: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::Subagents, async {
            match fs::remove_file(self.subagent_path(tid)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("subagents: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::Sandbox, async {
            match fs::remove_file(self.sandbox_path(tid)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("sandbox: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::ManageTasks, async {
            match fs::remove_file(self.manage_task_path(&tid.to_string())).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("manage_tasks: {e}"))),
            }
        });

        let art_dir = self.root.join("artifacts").join(tid.to_string());
        phase!(DeleteThreadPhase::Artifacts, async {
            match fs::remove_dir_all(&art_dir).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("artifacts: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::Uploads, async {
            match fs::remove_dir_all(self.uploads_dir(tid)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(e) => Err(StateError::Backend(format!("uploads: {e}"))),
            }
        });

        phase!(DeleteThreadPhase::ThreadMeta, async {
            self.delete_thread_meta_if_present(tid).await
        });

        let r = DeleteThreadReport {
            operation_id,
            thread_id: tid,
            status: DeleteThreadStatus::Complete,
            completed_phases: completed,
            consistency,
            retryable: true,
        };
        self.persist_lifecycle_audit_local_fs(&r).await?;
        Ok(r)
    }

    async fn last_delete_thread_report(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<DeleteThreadReport>, StateError> {
        let path = self.thread_ops_last_path(thread_id);
        match fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| StateError::Backend(format!("audit decode: {e}")))
                .map(Some),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StateError::Backend(format!("read audit: {e}"))),
        }
    }

    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteVerifyReport, StateError> {
        let tid = thread_id;
        let mut residual_by_phase = HashMap::new();
        let cp = self.root.join("state").join("checkpoints").join(tid.to_string());
        residual_by_phase.insert(
            DeleteThreadPhase::Checkpoints,
            fs::metadata(&cp).await.map(|m| m.is_dir()).unwrap_or(false),
        );
        residual_by_phase
            .insert(DeleteThreadPhase::Memory, fs::metadata(self.memory_path(tid)).await.is_ok());
        residual_by_phase
            .insert(DeleteThreadPhase::Tools, fs::metadata(self.tools_path(tid)).await.is_ok());
        residual_by_phase.insert(
            DeleteThreadPhase::Subagents,
            fs::metadata(self.subagent_path(tid)).await.is_ok(),
        );
        residual_by_phase
            .insert(DeleteThreadPhase::Sandbox, fs::metadata(self.sandbox_path(tid)).await.is_ok());
        residual_by_phase.insert(
            DeleteThreadPhase::ManageTasks,
            fs::metadata(self.manage_task_path(&tid.to_string())).await.is_ok(),
        );
        let art = self.root.join("artifacts").join(tid.to_string());
        residual_by_phase.insert(
            DeleteThreadPhase::Artifacts,
            fs::metadata(&art).await.map(|m| m.is_dir()).unwrap_or(false),
        );
        residual_by_phase.insert(
            DeleteThreadPhase::Uploads,
            fs::metadata(self.uploads_dir(tid)).await.map(|m| m.is_dir()).unwrap_or(false),
        );
        let in_meta = self
            .read_json_vec::<ThreadMeta>(&self.thread_meta_path())
            .await
            .map(|v| v.iter().any(|m| m.thread_id == tid))
            .unwrap_or(false);
        residual_by_phase.insert(DeleteThreadPhase::ThreadMeta, in_meta);
        Ok(DeleteVerifyReport { thread_id: tid, residual_by_phase })
    }
}

impl Default for LocalFsStateStore {
    fn default() -> Self {
        Self::new(".deer-flow/local-fs")
    }
}

pub fn now_utc() -> chrono::DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{
        ManageTaskStore, ThreadLifecycleStore, ThreadMetaStore, ThreadUploadStore,
    };

    fn test_store() -> LocalFsStateStore {
        let root =
            std::env::temp_dir().join(format!("open-harness-local-fs-store-{}", Uuid::new_v4()));
        LocalFsStateStore::new(root)
    }

    #[tokio::test]
    async fn delete_thread_cascade_removes_thread_scoped_data() {
        let store = test_store();
        let tid = Uuid::new_v4();
        let meta = ThreadMeta {
            thread_id: tid,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            label: None,
        };
        ThreadMetaStore::upsert_thread(&store, &meta).await.unwrap();
        store.put_upload(tid, "a.txt", b"hello").await.unwrap();
        ThreadLifecycleStore::delete_thread_cascade(&store, tid).await.unwrap();
        assert!(ThreadMetaStore::get_thread(&store, tid).await.is_err());
        assert!(store.get_upload(tid, "a.txt").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn manage_task_rejects_invalid_thread_id() {
        let store = test_store();
        let record = ManageTaskRecord {
            task_id: "task-1".to_string(),
            thread_id: "../unsafe".to_string(),
            status: "queued".to_string(),
            output_chunks: vec![],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "tenant-a".to_string(),
            user_id: "user-a".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        };
        let err = ManageTaskStore::upsert_task(&store, &record)
            .await
            .expect_err("must reject invalid thread id");
        assert!(matches!(err, StateError::Backend(_)));
    }
}
