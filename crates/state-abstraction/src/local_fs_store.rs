use agent_ports::{CheckpointRecord, RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use chrono::Utc;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

use crate::path_safety::sanitize_thread_id;
use crate::traits::{
    ArtifactStore, CheckpointStore, ManageTaskRecord, ManageTaskStore, MemoryStore,
    SandboxExecution, SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask,
    SubagentTaskStore, ThreadMeta, ThreadMetaStore, ToolRecord, ToolRecordStore,
};

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
}

#[async_trait]
impl MemoryStore for LocalFsStateStore {
    async fn append_fact(&self, thread_id: Uuid, fact: &str) -> Result<(), StateError> {
        let path = self.memory_path(thread_id);
        let mut items = self.read_json_vec::<String>(&path).await?;
        items.push(fact.to_string());
        self.write_json(&path, &items).await
    }

    async fn list_facts(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        self.read_json_vec::<String>(&self.memory_path(thread_id)).await
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
    use crate::traits::ManageTaskStore;

    fn test_store() -> LocalFsStateStore {
        let root =
            std::env::temp_dir().join(format!("open-harness-local-fs-store-{}", Uuid::new_v4()));
        LocalFsStateStore::new(root)
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
