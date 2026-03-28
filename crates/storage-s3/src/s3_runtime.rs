//! S3-compatible unified runtime state (JSON objects under a prefix).

use agent_ports::{RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use bytes::Bytes;
use futures::TryStreamExt;
use object_store::path::Path;
use object_store::Error as ObjectStoreError;
use object_store::ObjectStore;
use state_abstraction::{
    memory_document::{decode_memory_json_str, MemoryDocument},
    sanitize_thread_id, ArtifactStore, CheckpointRecord, CheckpointStore, DeleteConsistencyLevel,
    DeleteThreadPhase, DeleteThreadReport, DeleteThreadStatus, DeleteVerifyReport, ManageAppConfig,
    ManageConfigStore, ManageTaskRecord, ManageTaskStore, McpConfigStore, MemoryStore,
    SandboxExecution, SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask,
    SubagentTaskStore, ThreadLifecycleStore, ThreadMeta, ThreadMetaStore, ThreadUploadStore,
    ToolRecord, ToolRecordStore,
};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

fn require_thread_id(thread_id: &str) -> Result<(), StateError> {
    if sanitize_thread_id(thread_id).is_some() {
        Ok(())
    } else {
        Err(StateError::Backend(format!("invalid thread_id: {thread_id}")))
    }
}

/// Unified object-store backend for all runtime ports.
pub struct S3RuntimeStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl S3RuntimeStore {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: impl Into<String>) -> Self {
        Self { store, prefix: prefix.into().trim_matches('/').to_string() }
    }

    async fn get_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, StateError> {
        match self.store.get(path).await {
            Ok(g) => {
                Ok(Some(g.bytes().await.map_err(|e| StateError::Backend(e.to_string()))?.to_vec()))
            }
            Err(ObjectStoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(StateError::Backend(e.to_string())),
        }
    }

    async fn put_bytes(&self, path: &Path, bytes: Vec<u8>) -> Result<(), StateError> {
        self.store
            .put(path, Bytes::from(bytes).into())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
    ) -> Result<Option<T>, StateError> {
        let Some(b) = self.get_bytes(path).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&b).map_err(|e| StateError::Backend(format!("json: {e}"))).map(Some)
    }

    async fn put_json<T: serde::Serialize>(&self, path: &Path, v: &T) -> Result<(), StateError> {
        let bytes = serde_json::to_vec(v).map_err(|e| StateError::Backend(format!("json: {e}")))?;
        self.put_bytes(path, bytes).await
    }

    fn p(&self, tail: &str) -> Path {
        Path::from(format!("{}/runtime/v1/{}", self.prefix, tail))
    }

    fn task_path(&self, task_id: &str) -> Path {
        Path::from(format!("{}/manage_tasks/v1/tasks/{}.json", self.prefix, task_id))
    }

    fn thread_index_path(&self, thread_id: &str) -> Path {
        Path::from(format!("{}/manage_tasks/v1/thread_index/{}.json", self.prefix, thread_id))
    }

    async fn read_thread_ids(&self, thread_id: &str) -> Result<Vec<String>, StateError> {
        let path = self.thread_index_path(thread_id);
        let bytes = match self.store.get(&path).await {
            Ok(g) => g.bytes().await.map_err(|e| StateError::Backend(e.to_string()))?,
            Err(ObjectStoreError::NotFound { .. }) => return Ok(Vec::new()),
            Err(e) => return Err(StateError::Backend(e.to_string())),
        };
        let s = String::from_utf8(bytes.to_vec())
            .map_err(|e| StateError::Backend(format!("utf8: {e}")))?;
        let ids: Vec<String> = serde_json::from_str(&s).unwrap_or_else(|_| Vec::new());
        Ok(ids)
    }

    async fn write_thread_ids(&self, thread_id: &str, ids: &[String]) -> Result<(), StateError> {
        let path = self.thread_index_path(thread_id);
        let payload = serde_json::to_vec(ids).map_err(|e| StateError::Backend(e.to_string()))?;
        self.put_bytes(&path, payload).await
    }

    async fn delete_all_with_prefix(&self, tail: &str) -> Result<(), StateError> {
        let prefix = self.p(tail);
        let mut stream = self.store.list(Some(&prefix));
        while let Some(meta) =
            stream.try_next().await.map_err(|e| StateError::Backend(e.to_string()))?
        {
            match self.store.delete(&meta.location).await {
                Ok(()) => {}
                Err(ObjectStoreError::NotFound { .. }) => {}
                Err(e) => return Err(StateError::Backend(e.to_string())),
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ThreadMetaStore for S3RuntimeStore {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError> {
        let path = self.p(&format!("thread_meta/{}.json", meta.thread_id));
        self.put_json(&path, meta).await
    }

    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
        let path = self.p(&format!("thread_meta/{}.json", thread_id));
        self.get_json(&path).await?.ok_or_else(|| StateError::NotFound(thread_id.to_string()))
    }

    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError> {
        let path = self.p(&format!("thread_meta/{}.json", thread_id));
        match self.store.delete(&path).await {
            Ok(()) => Ok(()),
            Err(ObjectStoreError::NotFound { .. }) => {
                Err(StateError::NotFound(thread_id.to_string()))
            }
            Err(e) => Err(StateError::Backend(e.to_string())),
        }
    }
}

#[async_trait]
impl CheckpointStore for S3RuntimeStore {
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError> {
        let tid = record.thread_id.0;
        let rid = record.run_id.0;
        let step = record.step_seq.0;
        let latest = self.p(&format!("checkpoints/latest/{}/{}.json", tid, rid));
        let stepp = self.p(&format!("checkpoints/step/{}/{}/{}.json", tid, rid, step));
        self.put_json(&latest, record).await?;
        self.put_json(&stepp, record).await
    }

    async fn load_latest_checkpoint(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let path = self.p(&format!("checkpoints/latest/{}/{}.json", thread_id.0, run_id.0));
        self.get_json(&path).await
    }

    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let path =
            self.p(&format!("checkpoints/step/{}/{}/{}.json", thread_id.0, run_id.0, step_seq.0));
        self.get_json(&path).await
    }

    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError> {
        let prefix = self.p(&format!("checkpoints/step/{}/{}", thread_id.0, run_id.0));
        let mut stream = self.store.list(Some(&prefix));
        let mut steps = Vec::new();
        while let Some(meta) =
            stream.try_next().await.map_err(|e| StateError::Backend(e.to_string()))?
        {
            let Some(name) = meta.location.filename() else {
                continue;
            };
            let Some(n) = name.strip_suffix(".json") else {
                continue;
            };
            if let Ok(v) = n.parse::<u64>() {
                steps.push(StepSeq(v));
            }
        }
        steps.sort_by_key(|s| s.0);
        Ok(steps)
    }
}

#[async_trait]
impl ArtifactStore for S3RuntimeStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        let path = self.p(&format!("artifacts/{}/{}", thread_id, name));
        self.put_bytes(&path, bytes.to_vec()).await?;
        Ok(format!("s3:{}/{}/{}", self.prefix, thread_id, name))
    }

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let path = self.p(&format!("artifacts/{}/{}", thread_id, name));
        self.get_bytes(&path).await
    }
}

#[async_trait]
impl MemoryStore for S3RuntimeStore {
    async fn load_memory_document(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        let path = self.p(&format!("memory/{}.json", thread_id));
        let Some(raw) = self.get_bytes(&path).await? else {
            return Ok(MemoryDocument::default());
        };
        let s =
            String::from_utf8(raw).map_err(|e| StateError::Backend(format!("memory utf8: {e}")))?;
        decode_memory_json_str(&s).map_err(StateError::Backend)
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        let path = self.p(&format!("memory/{}.json", thread_id));
        let index_path = self.p("memory/index.json");
        if !doc.has_any_content() {
            match self.store.delete(&path).await {
                Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
                Err(e) => return Err(StateError::Backend(e.to_string())),
            }
            let mut ids: Vec<Uuid> = self.get_json(&index_path).await?.unwrap_or_default();
            ids.retain(|id| *id != thread_id);
            self.put_json(&index_path, &ids).await?;
            return Ok(());
        }
        self.put_json(&path, doc).await?;
        let mut ids: Vec<Uuid> = self.get_json(&index_path).await?.unwrap_or_default();
        if !ids.contains(&thread_id) {
            ids.push(thread_id);
            self.put_json(&index_path, &ids).await?;
        }
        Ok(())
    }

    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        let index_path = self.p("memory/index.json");
        let ids: Vec<Uuid> = self.get_json(&index_path).await?.unwrap_or_default();
        let mut out = Vec::new();
        for u in ids {
            if self.load_memory_document(u).await?.has_any_content() {
                out.push(u);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl SkillStore for S3RuntimeStore {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
        let path = self.p("skills.json");
        Ok(self.get_json(&path).await?.unwrap_or_default())
    }

    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError> {
        let mut skills = self.list_skills().await?;
        if let Some(s) = skills.iter_mut().find(|s| s.name == record.name) {
            *s = record.clone();
        } else {
            skills.push(record.clone());
        }
        let path = self.p("skills.json");
        self.put_json(&path, &skills).await
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError> {
        let skills = self.list_skills().await?;
        Ok(skills.into_iter().find(|s| s.name == name))
    }
}

#[async_trait]
impl ToolRecordStore for S3RuntimeStore {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError> {
        let path = self.p(&format!("tools/{}.json", record.thread_id));
        let mut items: Vec<ToolRecord> = self.get_json(&path).await?.unwrap_or_default();
        items.push(record.clone());
        self.put_json(&path, &items).await
    }

    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
        let path = self.p(&format!("tools/{}.json", thread_id));
        Ok(self.get_json(&path).await?.unwrap_or_default())
    }
}

#[async_trait]
impl SubagentTaskStore for S3RuntimeStore {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError> {
        let by_id = self.p(&format!("subagent/task/{}.json", task.task_id));
        self.put_json(&by_id, task).await?;
        let list_path = self.p(&format!("subagent/thread/{}.json", task.thread_id));
        let mut ids: Vec<Uuid> = self.get_json(&list_path).await?.unwrap_or_default();
        if !ids.contains(&task.task_id) {
            ids.push(task.task_id);
        }
        self.put_json(&list_path, &ids).await
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
        let path = self.p(&format!("subagent/task/{}.json", task_id));
        self.get_json(&path).await
    }

    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError> {
        let list_path = self.p(&format!("subagent/thread/{}.json", thread_id));
        let ids: Vec<Uuid> = self.get_json(&list_path).await?.unwrap_or_default();
        let mut out = Vec::new();
        for id in ids {
            if let Some(t) = SubagentTaskStore::get_task(self, id).await? {
                out.push(t);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl SandboxExecutionStore for S3RuntimeStore {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError> {
        let path = self.p(&format!("sandbox/{}.json", exec.thread_id));
        let mut items: Vec<SandboxExecution> = self.get_json(&path).await?.unwrap_or_default();
        items.push(exec.clone());
        self.put_json(&path, &items).await
    }

    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError> {
        let path = self.p(&format!("sandbox/{}.json", thread_id));
        Ok(self.get_json(&path).await?.unwrap_or_default())
    }
}

#[async_trait]
impl ManageTaskStore for S3RuntimeStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        require_thread_id(&task.thread_id)?;
        let path = self.task_path(&task.task_id);
        let payload = serde_json::to_vec(task).map_err(|e| StateError::Backend(e.to_string()))?;
        self.put_bytes(&path, payload).await?;

        let mut ids = self.read_thread_ids(&task.thread_id).await?;
        if !ids.iter().any(|x| x == &task.task_id) {
            ids.push(task.task_id.clone());
        }
        self.write_thread_ids(&task.thread_id, &ids).await
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let path = self.task_path(task_id);
        let bytes = match self.store.get(&path).await {
            Ok(g) => g.bytes().await.map_err(|e| StateError::Backend(e.to_string()))?,
            Err(ObjectStoreError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(StateError::Backend(e.to_string())),
        };
        let task: ManageTaskRecord = serde_json::from_slice(&bytes)
            .map_err(|e| StateError::Backend(format!("decode task: {e}")))?;
        Ok(Some(task))
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        require_thread_id(thread_id)?;
        let ids = self.read_thread_ids(thread_id).await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(t) = ManageTaskStore::get_task(self, &id).await? {
                out.push(t);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }
}

#[async_trait]
impl McpConfigStore for S3RuntimeStore {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
        let path = self.p("config/mcp_servers.json");
        Ok(self.get_json(&path).await?.unwrap_or_else(|| serde_json::json!({})))
    }

    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError> {
        let path = self.p("config/mcp_servers.json");
        self.put_json(&path, value).await
    }
}

#[async_trait]
impl ThreadUploadStore for S3RuntimeStore {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let marker = format!("uploads/{}/", thread_id);
        let prefix = self.p(&format!("uploads/{}/", thread_id));
        let mut stream = self.store.list(Some(&prefix));
        let mut out = Vec::new();
        while let Some(meta) =
            stream.try_next().await.map_err(|e| StateError::Backend(e.to_string()))?
        {
            let s = meta.location.as_ref();
            if let Some(i) = s.find(&marker) {
                let rest = &s[i + marker.len()..];
                if let Some(name) = rest.split('/').next() {
                    if !name.is_empty() {
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
        let path = self.p(&format!("uploads/{}/{}", thread_id, filename));
        self.put_bytes(&path, bytes.to_vec()).await
    }

    async fn get_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let path = self.p(&format!("uploads/{}/{}", thread_id, filename));
        self.get_bytes(&path).await
    }

    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError> {
        let path = self.p(&format!("uploads/{}/{}", thread_id, filename));
        match self.store.delete(&path).await {
            Ok(()) => Ok(()),
            Err(ObjectStoreError::NotFound { .. }) => Ok(()),
            Err(e) => Err(StateError::Backend(e.to_string())),
        }
    }
}

#[async_trait]
impl ManageConfigStore for S3RuntimeStore {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
        let path = self.p("config/manage_app.json");
        Ok(self.get_json(&path).await?.unwrap_or_default())
    }

    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError> {
        let path = self.p("config/manage_app.json");
        self.put_json(&path, cfg).await
    }
}

fn s3_partial_report(
    operation_id: Uuid,
    tid: Uuid,
    failed_at: DeleteThreadPhase,
    err: StateError,
    completed: &[DeleteThreadPhase],
    consistency: DeleteConsistencyLevel,
) -> DeleteThreadReport {
    DeleteThreadReport {
        operation_id,
        thread_id: tid,
        status: DeleteThreadStatus::Partial { failed_at, error: err.to_string() },
        completed_phases: completed.to_vec(),
        consistency,
        retryable: true,
    }
}

impl S3RuntimeStore {
    async fn persist_s3_audit(&self, report: &DeleteThreadReport) -> Result<(), StateError> {
        let path = self.p(&format!("ops/{}/last.json", report.thread_id));
        self.put_json(&path, report).await
    }

    /// `true` if the prefix lists at least one object.
    async fn prefix_has_any_object(&self, relative_prefix: &str) -> Result<bool, StateError> {
        let mut stream = self.store.list(Some(&self.p(relative_prefix)));
        match stream.try_next().await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(StateError::Backend(e.to_string())),
        }
    }
}

#[async_trait]
impl ThreadLifecycleStore for S3RuntimeStore {
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteThreadReport, StateError> {
        let operation_id = Uuid::new_v4();
        let tid = thread_id;
        let consistency = DeleteConsistencyLevel::BestEffort;
        let mut completed = Vec::new();

        if let Err(e) = self.delete_all_with_prefix(&format!("checkpoints/latest/{}/", tid)).await {
            let r = s3_partial_report(
                operation_id,
                tid,
                DeleteThreadPhase::Checkpoints,
                e,
                &completed,
                consistency,
            );
            let _ = self.persist_s3_audit(&r).await;
            return Ok(r);
        }
        if let Err(e) = self.delete_all_with_prefix(&format!("checkpoints/step/{}/", tid)).await {
            let r = s3_partial_report(
                operation_id,
                tid,
                DeleteThreadPhase::Checkpoints,
                e,
                &completed,
                consistency,
            );
            let _ = self.persist_s3_audit(&r).await;
            return Ok(r);
        }
        completed.push(DeleteThreadPhase::Checkpoints);

        let mem_path = self.p(&format!("memory/{}.json", tid));
        match self.store.delete(&mem_path).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::Memory,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        let index_path = self.p("memory/index.json");
        if let Some(mut ids) = self.get_json::<Vec<Uuid>>(&index_path).await? {
            let before = ids.len();
            ids.retain(|u| *u != tid);
            if ids.len() != before {
                if let Err(e) = self.put_json(&index_path, &ids).await {
                    let r = s3_partial_report(
                        operation_id,
                        tid,
                        DeleteThreadPhase::Memory,
                        e,
                        &completed,
                        consistency,
                    );
                    let _ = self.persist_s3_audit(&r).await;
                    return Ok(r);
                }
            }
        }
        completed.push(DeleteThreadPhase::Memory);

        let tm_path = self.p(&format!("thread_meta/{}.json", tid));
        match self.store.delete(&tm_path).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::ThreadMeta,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        completed.push(DeleteThreadPhase::ThreadMeta);

        let tools_path = self.p(&format!("tools/{}.json", tid));
        match self.store.delete(&tools_path).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::Tools,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        completed.push(DeleteThreadPhase::Tools);

        let list_path = self.p(&format!("subagent/thread/{}.json", tid));
        if let Some(ids) = self.get_json::<Vec<Uuid>>(&list_path).await? {
            for id in ids {
                let tp = self.p(&format!("subagent/task/{}.json", id));
                match self.store.delete(&tp).await {
                    Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
                    Err(e) => {
                        let r = s3_partial_report(
                            operation_id,
                            tid,
                            DeleteThreadPhase::Subagents,
                            StateError::Backend(e.to_string()),
                            &completed,
                            consistency,
                        );
                        let _ = self.persist_s3_audit(&r).await;
                        return Ok(r);
                    }
                }
            }
        }
        match self.store.delete(&list_path).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::Subagents,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        completed.push(DeleteThreadPhase::Subagents);

        let sb_path = self.p(&format!("sandbox/{}.json", tid));
        match self.store.delete(&sb_path).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::Sandbox,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        completed.push(DeleteThreadPhase::Sandbox);

        let ids = self.read_thread_ids(&tid.to_string()).await?;
        for id in ids {
            let tp = self.task_path(&id);
            match self.store.delete(&tp).await {
                Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
                Err(e) => {
                    let r = s3_partial_report(
                        operation_id,
                        tid,
                        DeleteThreadPhase::ManageTasks,
                        StateError::Backend(e.to_string()),
                        &completed,
                        consistency,
                    );
                    let _ = self.persist_s3_audit(&r).await;
                    return Ok(r);
                }
            }
        }
        match self.store.delete(&self.thread_index_path(&tid.to_string())).await {
            Ok(()) | Err(ObjectStoreError::NotFound { .. }) => {}
            Err(e) => {
                let r = s3_partial_report(
                    operation_id,
                    tid,
                    DeleteThreadPhase::ManageTasks,
                    StateError::Backend(e.to_string()),
                    &completed,
                    consistency,
                );
                let _ = self.persist_s3_audit(&r).await;
                return Ok(r);
            }
        }
        completed.push(DeleteThreadPhase::ManageTasks);

        if let Err(e) = self.delete_all_with_prefix(&format!("artifacts/{}/", tid)).await {
            let r = s3_partial_report(
                operation_id,
                tid,
                DeleteThreadPhase::Artifacts,
                e,
                &completed,
                consistency,
            );
            let _ = self.persist_s3_audit(&r).await;
            return Ok(r);
        }
        completed.push(DeleteThreadPhase::Artifacts);

        if let Err(e) = self.delete_all_with_prefix(&format!("uploads/{}/", tid)).await {
            let r = s3_partial_report(
                operation_id,
                tid,
                DeleteThreadPhase::Uploads,
                e,
                &completed,
                consistency,
            );
            let _ = self.persist_s3_audit(&r).await;
            return Ok(r);
        }
        completed.push(DeleteThreadPhase::Uploads);

        let r = DeleteThreadReport {
            operation_id,
            thread_id: tid,
            status: DeleteThreadStatus::Complete,
            completed_phases: completed,
            consistency,
            retryable: true,
        };
        self.persist_s3_audit(&r).await?;
        Ok(r)
    }

    async fn last_delete_thread_report(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<DeleteThreadReport>, StateError> {
        let path = self.p(&format!("ops/{}/last.json", thread_id));
        self.get_json(&path).await
    }

    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteVerifyReport, StateError> {
        let tid = thread_id;
        let mut residual_by_phase = HashMap::new();
        let cp_latest = self.prefix_has_any_object(&format!("checkpoints/latest/{}/", tid)).await?;
        let cp_step = self.prefix_has_any_object(&format!("checkpoints/step/{}/", tid)).await?;
        residual_by_phase.insert(DeleteThreadPhase::Checkpoints, cp_latest || cp_step);
        residual_by_phase.insert(
            DeleteThreadPhase::Memory,
            self.get_json::<serde_json::Value>(&self.p(&format!("memory/{}.json", tid)))
                .await?
                .is_some(),
        );
        residual_by_phase.insert(
            DeleteThreadPhase::ThreadMeta,
            self.get_json::<serde_json::Value>(&self.p(&format!("thread_meta/{}.json", tid)))
                .await?
                .is_some(),
        );
        residual_by_phase.insert(
            DeleteThreadPhase::Tools,
            self.get_json::<serde_json::Value>(&self.p(&format!("tools/{}.json", tid)))
                .await?
                .is_some(),
        );
        residual_by_phase.insert(
            DeleteThreadPhase::Subagents,
            self.get_json::<serde_json::Value>(&self.p(&format!("subagent/thread/{}.json", tid)))
                .await?
                .is_some(),
        );
        residual_by_phase.insert(
            DeleteThreadPhase::Sandbox,
            self.get_json::<serde_json::Value>(&self.p(&format!("sandbox/{}.json", tid)))
                .await?
                .is_some(),
        );
        let task_ids = self.read_thread_ids(&tid.to_string()).await?;
        residual_by_phase.insert(DeleteThreadPhase::ManageTasks, !task_ids.is_empty());
        residual_by_phase.insert(
            DeleteThreadPhase::Artifacts,
            self.prefix_has_any_object(&format!("artifacts/{}/", tid)).await?,
        );
        residual_by_phase.insert(
            DeleteThreadPhase::Uploads,
            self.prefix_has_any_object(&format!("uploads/{}/", tid)).await?,
        );
        Ok(DeleteVerifyReport { thread_id: tid, residual_by_phase })
    }
}
