//! Redis-backed unified runtime state (JSON values + binary artifacts).

use agent_ports::{RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
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
use uuid::Uuid;

const P: &str = "oh:rt:v1";

fn require_thread_id(thread_id: &str) -> Result<(), StateError> {
    if sanitize_thread_id(thread_id).is_some() {
        Ok(())
    } else {
        Err(StateError::Backend(format!("invalid thread_id: {thread_id}")))
    }
}

/// Unified Redis store for all runtime ports.
pub struct RedisRuntimeStore {
    conn: ConnectionManager,
}

impl RedisRuntimeStore {
    pub async fn connect(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    async fn scan_delete_match(&self, pattern: &str) -> Result<(), StateError> {
        let keys = self.scan_keys_match(pattern).await?;
        if keys.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.clone();
        for chunk in keys.chunks(500) {
            let _: () = conn.del(chunk).await.map_err(|e| StateError::Backend(e.to_string()))?;
        }
        Ok(())
    }

    async fn scan_keys_match(&self, pattern: &str) -> Result<Vec<String>, StateError> {
        storage_common::redis_scan_match(self.conn.clone(), pattern)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))
    }
}

#[async_trait]
impl ThreadMetaStore for RedisRuntimeStore {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError> {
        let key = format!("{}:tmeta:{}", P, meta.thread_id);
        let mut c = self.conn.clone();
        let payload =
            serde_json::to_string(meta).map_err(|e| StateError::Backend(format!("tmeta: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
        let key = format!("{}:tmeta:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Err(StateError::NotFound(thread_id.to_string()));
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("tmeta: {e}")))
    }

    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError> {
        let key = format!("{}:tmeta:{}", P, thread_id);
        let mut c = self.conn.clone();
        let n: i32 = c.del(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        if n == 0 {
            return Err(StateError::NotFound(thread_id.to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl CheckpointStore for RedisRuntimeStore {
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError> {
        let payload =
            serde_json::to_string(record).map_err(|e| StateError::Backend(format!("cp: {e}")))?;
        let tid = record.thread_id.0;
        let rid = record.run_id.0;
        let step = record.step_seq.0;
        let mut c = self.conn.clone();
        let k_latest = format!("{}:cp:latest:{}:{}", P, tid, rid);
        let k_step = format!("{}:cp:step:{}:{}:{}", P, tid, rid, step);
        let _: () =
            c.set(&k_latest, &payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () =
            c.set(&k_step, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn load_latest_checkpoint(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let k = format!("{}:cp:latest:{}:{}", P, thread_id.0, run_id.0);
        let mut c = self.conn.clone();
        let raw: Option<String> = c.get(k).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("cp: {e}")))
    }

    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let k = format!("{}:cp:step:{}:{}:{}", P, thread_id.0, run_id.0, step_seq.0);
        let mut c = self.conn.clone();
        let raw: Option<String> = c.get(k).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("cp: {e}")))
    }

    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError> {
        let pattern = format!("{}:cp:step:{}:{}:*", P, thread_id.0, run_id.0);
        let keys = self.scan_keys_match(&pattern).await?;
        let mut steps = Vec::new();
        for k in keys {
            if let Some(last) = k.rsplit(':').next() {
                if let Ok(n) = last.parse::<u64>() {
                    steps.push(StepSeq(n));
                }
            }
        }
        steps.sort_by_key(|s| s.0);
        steps.dedup_by_key(|s| s.0);
        Ok(steps)
    }
}

#[async_trait]
impl ArtifactStore for RedisRuntimeStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        let key = format!("{}:art:{}:{}", P, thread_id, name);
        let mut c = self.conn.clone();
        let _: () = c.set(key, bytes).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(format!("redis:artifact:{thread_id}/{name}"))
    }

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let key = format!("{}:art:{}:{}", P, thread_id, name);
        let mut c = self.conn.clone();
        let raw: Option<Vec<u8>> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(raw)
    }
}

#[async_trait]
impl MemoryStore for RedisRuntimeStore {
    async fn load_memory_document(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        let key = format!("{}:mem:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(MemoryDocument::default());
        };
        decode_memory_json_str(&raw).map_err(StateError::Backend)
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        let key = format!("{}:mem:{}", P, thread_id);
        let threads_key = format!("{}:mem:threads", P);
        let mut c = self.conn.clone();
        if !doc.has_any_content() {
            let _: () = c.del(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
            let _: () = c
                .srem(threads_key, thread_id.to_string())
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
            return Ok(());
        }
        let payload =
            serde_json::to_string(doc).map_err(|e| StateError::Backend(format!("mem: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () = c
            .sadd(threads_key, thread_id.to_string())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        let key = format!("{}:mem:threads", P);
        let mut c = self.conn.clone();
        let ids: Vec<String> =
            c.smembers(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for s in ids {
            let Ok(u) = Uuid::parse_str(&s) else {
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
impl SkillStore for RedisRuntimeStore {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
        let key = format!("{}:skills", P);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("skills: {e}")))
    }

    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError> {
        let mut skills = self.list_skills().await?;
        if let Some(s) = skills.iter_mut().find(|s| s.name == record.name) {
            *s = record.clone();
        } else {
            skills.push(record.clone());
        }
        let key = format!("{}:skills", P);
        let mut c = self.conn.clone();
        let payload = serde_json::to_string(&skills)
            .map_err(|e| StateError::Backend(format!("skills: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError> {
        let skills = self.list_skills().await?;
        Ok(skills.into_iter().find(|s| s.name == name))
    }
}

#[async_trait]
impl ToolRecordStore for RedisRuntimeStore {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError> {
        let key = format!("{}:tools:{}", P, record.thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut items: Vec<ToolRecord> =
            raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        items.push(record.clone());
        let payload = serde_json::to_string(&items)
            .map_err(|e| StateError::Backend(format!("tools: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
        let key = format!("{}:tools:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("tools: {e}")))
    }
}

#[async_trait]
impl SubagentTaskStore for RedisRuntimeStore {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError> {
        let by_id = format!("{}:sub:tid:{}", P, task.task_id);
        let mut c = self.conn.clone();
        let payload =
            serde_json::to_string(task).map_err(|e| StateError::Backend(format!("sub: {e}")))?;
        let _: () =
            c.set(&by_id, &payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let list_key = format!("{}:sub:thread:{}", P, task.thread_id);
        let raw: Option<String> =
            c.get(&list_key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut ids: Vec<Uuid> =
            raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if !ids.contains(&task.task_id) {
            ids.push(task.task_id);
        }
        let list_payload =
            serde_json::to_string(&ids).map_err(|e| StateError::Backend(format!("sub: {e}")))?;
        let _: () =
            c.set(list_key, list_payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
        let key = format!("{}:sub:tid:{}", P, task_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("sub: {e}")))
    }

    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError> {
        let list_key = format!("{}:sub:thread:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(&list_key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let ids: Vec<Uuid> = raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
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
impl SandboxExecutionStore for RedisRuntimeStore {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError> {
        let key = format!("{}:sbx:{}", P, exec.thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut items: Vec<SandboxExecution> =
            raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        items.push(exec.clone());
        let payload =
            serde_json::to_string(&items).map_err(|e| StateError::Backend(format!("sbx: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError> {
        let key = format!("{}:sbx:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("sbx: {e}")))
    }
}

#[async_trait]
impl ManageTaskStore for RedisRuntimeStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        require_thread_id(&task.thread_id)?;
        let payload = serde_json::to_string(task)
            .map_err(|e| StateError::Backend(format!("encode task: {e}")))?;
        let mut c = self.conn.clone();
        let tk = format!("{}:mt:task:{}", P, task.task_id);
        let th = format!("{}:mt:thread:{}", P, task.thread_id);
        let _: () = c.set(&tk, &payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () =
            c.sadd(&th, &task.task_id).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let mut c = self.conn.clone();
        let tk = format!("{}:mt:task:{}", P, task_id);
        let raw: Option<String> =
            c.get(tk).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let task: ManageTaskRecord = serde_json::from_str(&raw)
            .map_err(|e| StateError::Backend(format!("decode task: {e}")))?;
        Ok(Some(task))
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        require_thread_id(thread_id)?;
        let mut c = self.conn.clone();
        let th = format!("{}:mt:thread:{}", P, thread_id);
        let ids: Vec<String> =
            c.smembers(th).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(ids.len());
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
impl McpConfigStore for RedisRuntimeStore {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
        let key = format!("{}:app:mcp", P);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(serde_json::json!({}));
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("mcp: {e}")))
    }

    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError> {
        let key = format!("{}:app:mcp", P);
        let mut c = self.conn.clone();
        let payload =
            serde_json::to_string(value).map_err(|e| StateError::Backend(format!("mcp: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadUploadStore for RedisRuntimeStore {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let idx = format!("{}:up:files:{}", P, thread_id);
        let mut c = self.conn.clone();
        let names: Vec<String> =
            c.smembers(&idx).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out: Vec<String> = names.into_iter().collect();
        out.sort();
        Ok(out)
    }

    async fn put_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), StateError> {
        let key = format!("{}:up:{}:{}", P, thread_id, filename);
        let idx = format!("{}:up:files:{}", P, thread_id);
        let mut c = self.conn.clone();
        let _: () = c.set(key, bytes).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () = c.sadd(&idx, filename).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let key = format!("{}:up:{}:{}", P, thread_id, filename);
        let mut c = self.conn.clone();
        c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))
    }

    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError> {
        let key = format!("{}:up:{}:{}", P, thread_id, filename);
        let idx = format!("{}:up:files:{}", P, thread_id);
        let mut c = self.conn.clone();
        let _: () = c.del(&key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () = c.srem(&idx, filename).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ManageConfigStore for RedisRuntimeStore {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
        let key = format!("{}:app:manage", P);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(ManageAppConfig::default());
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("manage_app: {e}")))
    }

    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError> {
        let key = format!("{}:app:manage", P);
        let mut c = self.conn.clone();
        let payload = serde_json::to_string(cfg)
            .map_err(|e| StateError::Backend(format!("manage_app: {e}")))?;
        let _: () = c.set(key, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

impl RedisRuntimeStore {
    async fn persist_redis_audit(&self, report: &DeleteThreadReport) -> Result<(), StateError> {
        let key = format!("{}:op:last:{}", P, report.thread_id);
        let mut c = self.conn.clone();
        let payload =
            serde_json::to_string(report).map_err(|e| StateError::Backend(e.to_string()))?;
        let _: () = c
            .set_ex(&key, payload, 7 * 24 * 3600)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadLifecycleStore for RedisRuntimeStore {
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteThreadReport, StateError> {
        let operation_id = Uuid::new_v4();
        let tid = thread_id;
        let consistency = DeleteConsistencyLevel::BestEffort;
        let mut completed = Vec::new();
        let mut c = self.conn.clone();

        macro_rules! push_ok {
            ($phase:expr) => {
                completed.push($phase);
            };
        }

        let tmeta = format!("{}:tmeta:{}", P, tid);
        if let Err(e) = c.del::<_, ()>(&tmeta).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::ThreadMeta,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::ThreadMeta);

        if let Err(e) = self.scan_delete_match(&format!("{}:cp:latest:{}:*", P, tid)).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Checkpoints,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        if let Err(e) = self.scan_delete_match(&format!("{}:cp:step:{}:*", P, tid)).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Checkpoints,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Checkpoints);

        let mem = format!("{}:mem:{}", P, tid);
        if let Err(e) = c.del::<_, ()>(&mem).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Memory,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        let threads_key = format!("{}:mem:threads", P);
        let _: () = c
            .srem(threads_key, tid.to_string())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        push_ok!(DeleteThreadPhase::Memory);

        let tools = format!("{}:tools:{}", P, tid);
        if let Err(e) = c.del::<_, ()>(&tools).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Tools,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Tools);

        let list_key = format!("{}:sub:thread:{}", P, tid);
        let raw: Option<String> =
            c.get(&list_key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let ids: Vec<Uuid> = raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        for id in ids {
            let sub_id_key = format!("{}:sub:tid:{}", P, id);
            if let Err(e) = c.del::<_, ()>(&sub_id_key).await {
                let r = DeleteThreadReport {
                    operation_id,
                    thread_id: tid,
                    status: DeleteThreadStatus::Partial {
                        failed_at: DeleteThreadPhase::Subagents,
                        error: e.to_string(),
                    },
                    completed_phases: completed.clone(),
                    consistency,
                    retryable: true,
                };
                let _ = self.persist_redis_audit(&r).await;
                return Ok(r);
            }
        }
        if let Err(e) = c.del::<_, ()>(&list_key).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Subagents,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Subagents);

        let sbx = format!("{}:sbx:{}", P, tid);
        if let Err(e) = c.del::<_, ()>(&sbx).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Sandbox,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Sandbox);

        let th = format!("{}:mt:thread:{}", P, tid);
        let task_ids: Vec<String> =
            c.smembers(&th).await.map_err(|e| StateError::Backend(e.to_string()))?;
        for id in &task_ids {
            let tk = format!("{}:mt:task:{}", P, id);
            if let Err(e) = c.del::<_, ()>(&tk).await {
                let r = DeleteThreadReport {
                    operation_id,
                    thread_id: tid,
                    status: DeleteThreadStatus::Partial {
                        failed_at: DeleteThreadPhase::ManageTasks,
                        error: e.to_string(),
                    },
                    completed_phases: completed.clone(),
                    consistency,
                    retryable: true,
                };
                let _ = self.persist_redis_audit(&r).await;
                return Ok(r);
            }
        }
        if let Err(e) = c.del::<_, ()>(&th).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::ManageTasks,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::ManageTasks);

        if let Err(e) = self.scan_delete_match(&format!("{}:art:{}:*", P, tid)).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Artifacts,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Artifacts);

        let idx = format!("{}:up:files:{}", P, tid);
        let names: Vec<String> = c.smembers(&idx).await.unwrap_or_default();
        for name in names {
            let uk = format!("{}:up:{}:{}", P, tid, name);
            if let Err(e) = c.del::<_, ()>(&uk).await {
                let r = DeleteThreadReport {
                    operation_id,
                    thread_id: tid,
                    status: DeleteThreadStatus::Partial {
                        failed_at: DeleteThreadPhase::Uploads,
                        error: e.to_string(),
                    },
                    completed_phases: completed.clone(),
                    consistency,
                    retryable: true,
                };
                let _ = self.persist_redis_audit(&r).await;
                return Ok(r);
            }
        }
        if let Err(e) = c.del::<_, ()>(&idx).await {
            let r = DeleteThreadReport {
                operation_id,
                thread_id: tid,
                status: DeleteThreadStatus::Partial {
                    failed_at: DeleteThreadPhase::Uploads,
                    error: e.to_string(),
                },
                completed_phases: completed.clone(),
                consistency,
                retryable: true,
            };
            let _ = self.persist_redis_audit(&r).await;
            return Ok(r);
        }
        push_ok!(DeleteThreadPhase::Uploads);

        let r = DeleteThreadReport {
            operation_id,
            thread_id: tid,
            status: DeleteThreadStatus::Complete,
            completed_phases: completed,
            consistency,
            retryable: true,
        };
        self.persist_redis_audit(&r).await?;
        Ok(r)
    }

    async fn last_delete_thread_report(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<DeleteThreadReport>, StateError> {
        let key = format!("{}:op:last:{}", P, thread_id);
        let mut c = self.conn.clone();
        let raw: Option<String> =
            c.get(key).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map_err(|e| StateError::Backend(format!("audit: {e}")))
    }

    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteVerifyReport, StateError> {
        let tid = thread_id;
        let mut c = self.conn.clone();
        let mut residual_by_phase = HashMap::new();
        let n_cp = self.scan_keys_match(&format!("{}:cp:latest:{}:*", P, tid)).await?.len()
            + self.scan_keys_match(&format!("{}:cp:step:{}:*", P, tid)).await?.len();
        residual_by_phase.insert(DeleteThreadPhase::Checkpoints, n_cp > 0);
        let mem = format!("{}:mem:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::Memory,
            c.exists(&mem).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        let tools = format!("{}:tools:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::Tools,
            c.exists(&tools).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        let list_key = format!("{}:sub:thread:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::Subagents,
            c.exists(&list_key).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        let sbx = format!("{}:sbx:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::Sandbox,
            c.exists(&sbx).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        let th = format!("{}:mt:thread:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::ManageTasks,
            c.exists(&th).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        let n_art = self.scan_keys_match(&format!("{}:art:{}:*", P, tid)).await?.len();
        residual_by_phase.insert(DeleteThreadPhase::Artifacts, n_art > 0);
        let idx = format!("{}:up:files:{}", P, tid);
        let up_nonempty =
            c.scard::<_, u64>(&idx).await.map_err(|e| StateError::Backend(e.to_string()))? > 0;
        residual_by_phase.insert(DeleteThreadPhase::Uploads, up_nonempty);
        let tmeta = format!("{}:tmeta:{}", P, tid);
        residual_by_phase.insert(
            DeleteThreadPhase::ThreadMeta,
            c.exists(&tmeta).await.map_err(|e| StateError::Backend(e.to_string()))?,
        );
        Ok(DeleteVerifyReport { thread_id: tid, residual_by_phase })
    }
}
