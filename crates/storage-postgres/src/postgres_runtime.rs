//! Single PostgreSQL database implementing all runtime state ports.

use agent_ports::{RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use state_abstraction::{
    memory_document::{decode_memory_json_value, MemoryDocument},
    ArtifactStore, CheckpointRecord, CheckpointStore, DeleteConsistencyLevel, DeleteThreadPhase,
    DeleteThreadReport, DeleteThreadStatus, DeleteVerifyReport, ManageAppConfig, ManageConfigStore,
    ManageTaskRecord, ManageTaskStore, McpConfigStore, MemoryStore, SandboxExecution,
    SandboxExecutionStore, SkillRecord, SkillStore, StateError, SubagentTask, SubagentTaskStore,
    ThreadLifecycleStore, ThreadMeta, ThreadMetaStore, ThreadUploadStore, ToolRecord,
    ToolRecordStore,
};
use std::collections::HashMap;
use uuid::Uuid;

use state_abstraction::sanitize_thread_id;

fn require_thread_id(thread_id: &str) -> Result<(), StateError> {
    if sanitize_thread_id(thread_id).is_some() {
        Ok(())
    } else {
        Err(StateError::Backend(format!("invalid thread_id: {thread_id}")))
    }
}

#[derive(Debug, Clone)]
pub struct PostgresRuntimeStore {
    pool: PgPool,
}

impl PostgresRuntimeStore {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS thread_meta (
                thread_id UUID PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                label TEXT
            );
            CREATE TABLE IF NOT EXISTS manage_tasks (
                task_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output_chunks JSONB NOT NULL,
                error TEXT,
                callback_url TEXT,
                stream BOOLEAN NOT NULL,
                client_task_id TEXT,
                tenant_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                version BIGINT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_manage_tasks_thread_id ON manage_tasks (thread_id);
            CREATE TABLE IF NOT EXISTS checkpoints_latest (
                thread_id UUID NOT NULL,
                run_id UUID NOT NULL,
                payload JSONB NOT NULL,
                PRIMARY KEY (thread_id, run_id)
            );
            CREATE TABLE IF NOT EXISTS checkpoints_step (
                thread_id UUID NOT NULL,
                run_id UUID NOT NULL,
                step_seq BIGINT NOT NULL,
                payload JSONB NOT NULL,
                PRIMARY KEY (thread_id, run_id, step_seq)
            );
            CREATE TABLE IF NOT EXISTS memory_facts (
                thread_id UUID PRIMARY KEY,
                facts_json JSONB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS skills (
                name TEXT PRIMARY KEY,
                enabled BOOLEAN NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tool_records (
                id BIGSERIAL PRIMARY KEY,
                thread_id UUID NOT NULL,
                record_json JSONB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tool_records_thread ON tool_records (thread_id);
            CREATE TABLE IF NOT EXISTS subagent_tasks (
                task_id UUID PRIMARY KEY NOT NULL,
                thread_id UUID NOT NULL,
                record_json JSONB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_subagent_thread ON subagent_tasks (thread_id);
            CREATE TABLE IF NOT EXISTS sandbox_executions (
                id BIGSERIAL PRIMARY KEY,
                thread_id UUID NOT NULL,
                record_json JSONB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sandbox_thread ON sandbox_executions (thread_id);
            CREATE TABLE IF NOT EXISTS artifacts (
                thread_id UUID NOT NULL,
                name TEXT NOT NULL,
                bytes BYTEA NOT NULL,
                PRIMARY KEY (thread_id, name)
            );
            CREATE TABLE IF NOT EXISTS app_kv (
                key TEXT PRIMARY KEY,
                value JSONB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS thread_uploads (
                thread_id UUID NOT NULL,
                filename TEXT NOT NULL,
                bytes BYTEA NOT NULL,
                PRIMARY KEY (thread_id, filename)
            );
            CREATE TABLE IF NOT EXISTS thread_lifecycle_ops (
                op_id UUID PRIMARY KEY NOT NULL,
                thread_id UUID NOT NULL,
                report_json JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_lifecycle_ops_thread ON thread_lifecycle_ops (thread_id);
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl ThreadMetaStore for PostgresRuntimeStore {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError> {
        sqlx::query(
            r#"
            INSERT INTO thread_meta (thread_id, created_at, updated_at, label)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (thread_id) DO UPDATE SET
                updated_at = EXCLUDED.updated_at,
                label = EXCLUDED.label
            "#,
        )
        .bind(meta.thread_id)
        .bind(meta.created_at)
        .bind(meta.updated_at)
        .bind(&meta.label)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
        let row: Option<(Uuid, DateTime<Utc>, DateTime<Utc>, Option<String>)> = sqlx::query_as(
            "SELECT thread_id, created_at, updated_at, label FROM thread_meta WHERE thread_id = $1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let Some((tid, ca, ua, label)) = row else {
            return Err(StateError::NotFound(thread_id.to_string()));
        };
        Ok(ThreadMeta { thread_id: tid, created_at: ca, updated_at: ua, label })
    }

    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError> {
        let r = sqlx::query("DELETE FROM thread_meta WHERE thread_id = $1")
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        if r.rows_affected() == 0 {
            return Err(StateError::NotFound(thread_id.to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl CheckpointStore for PostgresRuntimeStore {
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError> {
        let payload: Value = serde_json::to_value(record)
            .map_err(|e| StateError::Backend(format!("checkpoint encode: {e}")))?;
        let tid = record.thread_id.0;
        let rid = record.run_id.0;
        let step = record.step_seq.0 as i64;
        sqlx::query(
            r#"
            INSERT INTO checkpoints_step (thread_id, run_id, step_seq, payload)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (thread_id, run_id, step_seq) DO UPDATE SET payload = EXCLUDED.payload
            "#,
        )
        .bind(tid)
        .bind(rid)
        .bind(step)
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO checkpoints_latest (thread_id, run_id, payload)
            VALUES ($1, $2, $3)
            ON CONFLICT (thread_id, run_id) DO UPDATE SET payload = EXCLUDED.payload
            "#,
        )
        .bind(tid)
        .bind(rid)
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn load_latest_checkpoint(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let row: Option<(Value,)> = sqlx::query_as(
            "SELECT payload FROM checkpoints_latest WHERE thread_id = $1 AND run_id = $2",
        )
        .bind(thread_id.0)
        .bind(run_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((payload,)) = row else {
            return Ok(None);
        };
        serde_json::from_value(payload).map_err(|e| StateError::Backend(format!("checkpoint: {e}")))
    }

    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let row: Option<(Value,)> = sqlx::query_as(
            "SELECT payload FROM checkpoints_step WHERE thread_id = $1 AND run_id = $2 AND step_seq = $3",
        )
        .bind(thread_id.0)
        .bind(run_id.0)
        .bind(step_seq.0 as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((payload,)) = row else {
            return Ok(None);
        };
        serde_json::from_value(payload).map_err(|e| StateError::Backend(format!("checkpoint: {e}")))
    }

    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT step_seq FROM checkpoints_step WHERE thread_id = $1 AND run_id = $2 ORDER BY step_seq ASC",
        )
        .bind(thread_id.0)
        .bind(run_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(rows.into_iter().map(|(s,)| StepSeq(s as u64)).collect())
    }
}

#[async_trait]
impl ArtifactStore for PostgresRuntimeStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        sqlx::query(
            r#"
            INSERT INTO artifacts (thread_id, name, bytes) VALUES ($1, $2, $3)
            ON CONFLICT (thread_id, name) DO UPDATE SET bytes = EXCLUDED.bytes
            "#,
        )
        .bind(thread_id)
        .bind(name)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(format!("postgres:artifact:{thread_id}/{name}"))
    }

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT bytes FROM artifacts WHERE thread_id = $1 AND name = $2")
                .bind(thread_id)
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(b,)| b))
    }
}

#[async_trait]
impl MemoryStore for PostgresRuntimeStore {
    async fn load_memory_document(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT facts_json FROM memory_facts WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((v,)) = row else {
            return Ok(MemoryDocument::default());
        };
        decode_memory_json_value(&v).map_err(StateError::Backend)
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        if !doc.has_any_content() {
            sqlx::query("DELETE FROM memory_facts WHERE thread_id = $1")
                .bind(thread_id)
                .execute(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
            return Ok(());
        }
        let facts_json =
            serde_json::to_value(doc).map_err(|e| StateError::Backend(format!("memory: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO memory_facts (thread_id, facts_json) VALUES ($1, $2)
            ON CONFLICT (thread_id) DO UPDATE SET facts_json = EXCLUDED.facts_json
            "#,
        )
        .bind(thread_id)
        .bind(facts_json)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        let rows: Vec<(Uuid, Value)> =
            sqlx::query_as("SELECT thread_id, facts_json FROM memory_facts")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for (u, v) in rows {
            if decode_memory_json_value(&v).map_err(StateError::Backend)?.has_any_content() {
                out.push(u);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl SkillStore for PostgresRuntimeStore {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
        let rows: Vec<(String, bool)> =
            sqlx::query_as("SELECT name, enabled FROM skills ORDER BY name")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(rows.into_iter().map(|(name, enabled)| SkillRecord { name, enabled }).collect())
    }

    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError> {
        sqlx::query(
            r#"
            INSERT INTO skills (name, enabled) VALUES ($1, $2)
            ON CONFLICT (name) DO UPDATE SET enabled = EXCLUDED.enabled
            "#,
        )
        .bind(&record.name)
        .bind(record.enabled)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError> {
        let row: Option<(String, bool)> =
            sqlx::query_as("SELECT name, enabled FROM skills WHERE name = $1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(n, en)| SkillRecord { name: n, enabled: en }))
    }
}

#[async_trait]
impl ToolRecordStore for PostgresRuntimeStore {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError> {
        let j =
            serde_json::to_value(record).map_err(|e| StateError::Backend(format!("tool: {e}")))?;
        sqlx::query("INSERT INTO tool_records (thread_id, record_json) VALUES ($1, $2)")
            .bind(record.thread_id)
            .bind(j)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
        let rows: Vec<(Value,)> =
            sqlx::query_as("SELECT record_json FROM tool_records WHERE thread_id = $1 ORDER BY id")
                .bind(thread_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (v,) in rows {
            let r: ToolRecord =
                serde_json::from_value(v).map_err(|e| StateError::Backend(format!("tool: {e}")))?;
            out.push(r);
        }
        Ok(out)
    }
}

#[async_trait]
impl SubagentTaskStore for PostgresRuntimeStore {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError> {
        let j = serde_json::to_value(task)
            .map_err(|e| StateError::Backend(format!("subagent: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO subagent_tasks (task_id, thread_id, record_json) VALUES ($1, $2, $3)
            ON CONFLICT (task_id) DO UPDATE SET thread_id = EXCLUDED.thread_id, record_json = EXCLUDED.record_json
            "#,
        )
        .bind(task.task_id)
        .bind(task.thread_id)
        .bind(j)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT record_json FROM subagent_tasks WHERE task_id = $1")
                .bind(task_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((v,)) = row else {
            return Ok(None);
        };
        serde_json::from_value(v).map_err(|e| StateError::Backend(format!("subagent: {e}")))
    }

    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError> {
        let rows: Vec<(Value,)> = sqlx::query_as(
            "SELECT record_json FROM subagent_tasks WHERE thread_id = $1 ORDER BY task_id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (v,) in rows {
            let t: SubagentTask = serde_json::from_value(v)
                .map_err(|e| StateError::Backend(format!("subagent: {e}")))?;
            out.push(t);
        }
        Ok(out)
    }
}

#[async_trait]
impl SandboxExecutionStore for PostgresRuntimeStore {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError> {
        let j =
            serde_json::to_value(exec).map_err(|e| StateError::Backend(format!("sandbox: {e}")))?;
        sqlx::query("INSERT INTO sandbox_executions (thread_id, record_json) VALUES ($1, $2)")
            .bind(exec.thread_id)
            .bind(j)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError> {
        let rows: Vec<(Value,)> = sqlx::query_as(
            "SELECT record_json FROM sandbox_executions WHERE thread_id = $1 ORDER BY id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (v,) in rows {
            let e: SandboxExecution = serde_json::from_value(v)
                .map_err(|e| StateError::Backend(format!("sandbox: {e}")))?;
            out.push(e);
        }
        Ok(out)
    }
}

#[async_trait]
impl ManageTaskStore for PostgresRuntimeStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        require_thread_id(&task.thread_id)?;
        let output_chunks = serde_json::to_value(&task.output_chunks)
            .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO manage_tasks (
                task_id, thread_id, status, output_chunks, error, callback_url, stream,
                client_task_id, tenant_id, user_id, created_at, updated_at, version
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
            ON CONFLICT (task_id) DO UPDATE SET
                thread_id = EXCLUDED.thread_id,
                status = EXCLUDED.status,
                output_chunks = EXCLUDED.output_chunks,
                error = EXCLUDED.error,
                callback_url = EXCLUDED.callback_url,
                stream = EXCLUDED.stream,
                client_task_id = EXCLUDED.client_task_id,
                tenant_id = EXCLUDED.tenant_id,
                user_id = EXCLUDED.user_id,
                created_at = EXCLUDED.created_at,
                updated_at = EXCLUDED.updated_at,
                version = EXCLUDED.version
            "#,
        )
        .bind(&task.task_id)
        .bind(&task.thread_id)
        .bind(&task.status)
        .bind(output_chunks)
        .bind(&task.error)
        .bind(&task.callback_url)
        .bind(task.stream)
        .bind(&task.client_task_id)
        .bind(&task.tenant_id)
        .bind(&task.user_id)
        .bind(task.created_at)
        .bind(task.updated_at)
        .bind(task.version)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let row: Option<(
            String,
            String,
            String,
            Value,
            Option<String>,
            Option<String>,
            bool,
            Option<String>,
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            i64,
        )> = sqlx::query_as(
            r#"
            SELECT task_id, thread_id, status, output_chunks, error, callback_url, stream,
                   client_task_id, tenant_id, user_id, created_at, updated_at, version
            FROM manage_tasks
            WHERE task_id = $1
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let output_chunks: Vec<String> = serde_json::from_value(row.3)
            .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
        Ok(Some(ManageTaskRecord {
            task_id: row.0,
            thread_id: row.1,
            status: row.2,
            output_chunks,
            error: row.4,
            callback_url: row.5,
            stream: row.6,
            client_task_id: row.7,
            tenant_id: row.8,
            user_id: row.9,
            created_at: row.10,
            updated_at: row.11,
            version: row.12,
        }))
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        require_thread_id(thread_id)?;
        let rows: Vec<(
            String,
            String,
            String,
            Value,
            Option<String>,
            Option<String>,
            bool,
            Option<String>,
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            i64,
        )> = sqlx::query_as(
            r#"
            SELECT task_id, thread_id, status, output_chunks, error, callback_url, stream,
                   client_task_id, tenant_id, user_id, created_at, updated_at, version
            FROM manage_tasks
            WHERE thread_id = $1
            ORDER BY updated_at DESC
            "#,
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let output_chunks: Vec<String> = serde_json::from_value(row.3)
                .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
            out.push(ManageTaskRecord {
                task_id: row.0,
                thread_id: row.1,
                status: row.2,
                output_chunks,
                error: row.4,
                callback_url: row.5,
                stream: row.6,
                client_task_id: row.7,
                tenant_id: row.8,
                user_id: row.9,
                created_at: row.10,
                updated_at: row.11,
                version: row.12,
            });
        }
        Ok(out)
    }
}

#[async_trait]
impl McpConfigStore for PostgresRuntimeStore {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT value FROM app_kv WHERE key = 'mcp_servers'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((v,)) = row else {
            return Ok(serde_json::json!({}));
        };
        Ok(v)
    }

    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError> {
        sqlx::query(
            r#"
            INSERT INTO app_kv (key, value) VALUES ('mcp_servers', $1)
            ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
            "#,
        )
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadUploadStore for PostgresRuntimeStore {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT filename FROM thread_uploads WHERE thread_id = $1 ORDER BY filename",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    async fn put_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(), StateError> {
        sqlx::query(
            r#"
            INSERT INTO thread_uploads (thread_id, filename, bytes) VALUES ($1, $2, $3)
            ON CONFLICT (thread_id, filename) DO UPDATE SET bytes = EXCLUDED.bytes
            "#,
        )
        .bind(thread_id)
        .bind(filename)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_upload(
        &self,
        thread_id: Uuid,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT bytes FROM thread_uploads WHERE thread_id = $1 AND filename = $2",
        )
        .bind(thread_id)
        .bind(filename)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(b,)| b))
    }

    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError> {
        sqlx::query("DELETE FROM thread_uploads WHERE thread_id = $1 AND filename = $2")
            .bind(thread_id)
            .bind(filename)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ManageConfigStore for PostgresRuntimeStore {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
        let row: Option<(Value,)> =
            sqlx::query_as("SELECT value FROM app_kv WHERE key = 'manage_app'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((v,)) = row else {
            return Ok(ManageAppConfig::default());
        };
        serde_json::from_value(v).map_err(|e| StateError::Backend(format!("manage_app: {e}")))
    }

    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError> {
        let v = serde_json::to_value(cfg)
            .map_err(|e| StateError::Backend(format!("manage_app: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO app_kv (key, value) VALUES ('manage_app', $1)
            ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value
            "#,
        )
        .bind(v)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

impl PostgresRuntimeStore {
    async fn persist_lifecycle_op_pg(&self, report: &DeleteThreadReport) -> Result<(), StateError> {
        let v = serde_json::to_value(report).map_err(|e| StateError::Backend(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO thread_lifecycle_ops (op_id, thread_id, report_json, created_at)
            VALUES ($1, $2, $3, NOW())
            "#,
        )
        .bind(report.operation_id)
        .bind(report.thread_id)
        .bind(v)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadLifecycleStore for PostgresRuntimeStore {
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteThreadReport, StateError> {
        let operation_id = Uuid::new_v4();
        let tid_str = thread_id.to_string();
        let consistency = DeleteConsistencyLevel::StrongPerThread;
        let mut tx = self.pool.begin().await.map_err(|e| StateError::Backend(e.to_string()))?;

        macro_rules! del {
            ($phase:expr, $q:literal) => {
                if let Err(e) = sqlx::query($q).bind(thread_id).execute(&mut *tx).await {
                    let _ = tx.rollback().await;
                    let r = DeleteThreadReport {
                        operation_id,
                        thread_id,
                        status: DeleteThreadStatus::Failed { at: $phase, error: e.to_string() },
                        completed_phases: vec![],
                        consistency,
                        retryable: true,
                    };
                    let _ = self.persist_lifecycle_op_pg(&r).await;
                    return Ok(r);
                }
            };
        }

        del!(DeleteThreadPhase::Checkpoints, "DELETE FROM checkpoints_latest WHERE thread_id = $1");
        del!(DeleteThreadPhase::Checkpoints, "DELETE FROM checkpoints_step WHERE thread_id = $1");
        del!(DeleteThreadPhase::Memory, "DELETE FROM memory_facts WHERE thread_id = $1");
        del!(DeleteThreadPhase::Tools, "DELETE FROM tool_records WHERE thread_id = $1");
        del!(DeleteThreadPhase::Subagents, "DELETE FROM subagent_tasks WHERE thread_id = $1");
        del!(DeleteThreadPhase::Sandbox, "DELETE FROM sandbox_executions WHERE thread_id = $1");
        if let Err(e) = sqlx::query("DELETE FROM manage_tasks WHERE thread_id = $1")
            .bind(&tid_str)
            .execute(&mut *tx)
            .await
        {
            let _ = tx.rollback().await;
            let r = DeleteThreadReport {
                operation_id,
                thread_id,
                status: DeleteThreadStatus::Failed {
                    at: DeleteThreadPhase::ManageTasks,
                    error: e.to_string(),
                },
                completed_phases: vec![],
                consistency,
                retryable: true,
            };
            let _ = self.persist_lifecycle_op_pg(&r).await;
            return Ok(r);
        }
        del!(DeleteThreadPhase::Artifacts, "DELETE FROM artifacts WHERE thread_id = $1");
        del!(DeleteThreadPhase::Uploads, "DELETE FROM thread_uploads WHERE thread_id = $1");
        del!(DeleteThreadPhase::ThreadMeta, "DELETE FROM thread_meta WHERE thread_id = $1");

        let completed = vec![
            DeleteThreadPhase::Checkpoints,
            DeleteThreadPhase::Memory,
            DeleteThreadPhase::Tools,
            DeleteThreadPhase::Subagents,
            DeleteThreadPhase::Sandbox,
            DeleteThreadPhase::ManageTasks,
            DeleteThreadPhase::Artifacts,
            DeleteThreadPhase::Uploads,
            DeleteThreadPhase::ThreadMeta,
        ];
        let report = DeleteThreadReport {
            operation_id,
            thread_id,
            status: DeleteThreadStatus::Complete,
            completed_phases: completed.clone(),
            consistency,
            retryable: true,
        };
        let json = match serde_json::to_value(&report) {
            Ok(j) => j,
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(StateError::Backend(e.to_string()));
            }
        };
        sqlx::query(
            r#"
            INSERT INTO thread_lifecycle_ops (op_id, thread_id, report_json, created_at)
            VALUES ($1, $2, $3, NOW())
            "#,
        )
        .bind(operation_id)
        .bind(thread_id)
        .bind(json)
        .execute(&mut *tx)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        tx.commit().await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(report)
    }

    async fn last_delete_thread_report(
        &self,
        thread_id: Uuid,
    ) -> Result<Option<DeleteThreadReport>, StateError> {
        let row: Option<(serde_json::Value,)> =
            sqlx::query_as("SELECT report_json FROM thread_lifecycle_ops WHERE thread_id = $1 ORDER BY created_at DESC LIMIT 1")
                .bind(thread_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((v,)) = row else {
            return Ok(None);
        };
        serde_json::from_value(v).map_err(|e| StateError::Backend(format!("report json: {e}")))
    }

    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteVerifyReport, StateError> {
        let tid_str = thread_id.to_string();
        let mut residual_by_phase = HashMap::new();
        let c: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM checkpoints_latest WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let c2: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM checkpoints_step WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Checkpoints, c.0 + c2.0 > 0);
        let m: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_facts WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Memory, m.0 > 0);
        let t: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tool_records WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Tools, t.0 > 0);
        let s: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM subagent_tasks WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Subagents, s.0 > 0);
        let sb: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sandbox_executions WHERE thread_id = $1")
                .bind(thread_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Sandbox, sb.0 > 0);
        let mt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM manage_tasks WHERE thread_id = $1")
            .bind(&tid_str)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::ManageTasks, mt.0 > 0);
        let a: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM artifacts WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Artifacts, a.0 > 0);
        let u: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM thread_uploads WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Uploads, u.0 > 0);
        let tm: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM thread_meta WHERE thread_id = $1")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::ThreadMeta, tm.0 > 0);
        Ok(DeleteVerifyReport { thread_id, residual_by_phase })
    }
}
