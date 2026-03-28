//! Single SQLite database implementing all runtime state ports (parity with `LocalFsStateStore`).

use agent_ports::{RunId, StepSeq, ThreadId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
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

/// All runtime durable state in one SQLite file.
#[derive(Debug, Clone)]
pub struct SqliteRuntimeStore {
    pool: SqlitePool,
}

impl SqliteRuntimeStore {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        Self::migrate(&pool).await?;
        Ok(Self { pool })
    }

    async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS thread_meta (
                thread_id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                label TEXT
            );
            CREATE TABLE IF NOT EXISTS manage_tasks (
                task_id TEXT PRIMARY KEY NOT NULL,
                thread_id TEXT NOT NULL,
                status TEXT NOT NULL,
                output_chunks TEXT NOT NULL,
                error TEXT,
                callback_url TEXT,
                stream INTEGER NOT NULL,
                client_task_id TEXT,
                tenant_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                version INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_manage_tasks_thread_id ON manage_tasks (thread_id);
            CREATE TABLE IF NOT EXISTS checkpoints_latest (
                thread_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY (thread_id, run_id)
            );
            CREATE TABLE IF NOT EXISTS checkpoints_step (
                thread_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                step_seq INTEGER NOT NULL,
                payload TEXT NOT NULL,
                PRIMARY KEY (thread_id, run_id, step_seq)
            );
            CREATE TABLE IF NOT EXISTS memory_facts (
                thread_id TEXT PRIMARY KEY,
                facts_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS skills (
                name TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tool_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                record_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tool_records_thread ON tool_records (thread_id);
            CREATE TABLE IF NOT EXISTS subagent_tasks (
                task_id TEXT PRIMARY KEY NOT NULL,
                thread_id TEXT NOT NULL,
                record_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_subagent_thread ON subagent_tasks (thread_id);
            CREATE TABLE IF NOT EXISTS sandbox_executions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                record_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sandbox_thread ON sandbox_executions (thread_id);
            CREATE TABLE IF NOT EXISTS artifacts (
                thread_id TEXT NOT NULL,
                name TEXT NOT NULL,
                bytes BLOB NOT NULL,
                PRIMARY KEY (thread_id, name)
            );
            CREATE TABLE IF NOT EXISTS app_kv (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS thread_uploads (
                thread_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                bytes BLOB NOT NULL,
                PRIMARY KEY (thread_id, filename)
            );
            CREATE TABLE IF NOT EXISTS thread_lifecycle_ops (
                op_id TEXT PRIMARY KEY NOT NULL,
                thread_id TEXT NOT NULL,
                report_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_thread_lifecycle_ops_thread ON thread_lifecycle_ops (thread_id);
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    fn require_thread_id(thread_id: &str) -> Result<(), StateError> {
        if sanitize_thread_id(thread_id).is_some() {
            Ok(())
        } else {
            Err(StateError::Backend(format!("invalid thread_id: {thread_id}")))
        }
    }
}

#[async_trait]
impl ThreadMetaStore for SqliteRuntimeStore {
    async fn upsert_thread(&self, meta: &ThreadMeta) -> Result<(), StateError> {
        let id = meta.thread_id.to_string();
        let created = meta.created_at.to_rfc3339();
        let updated = meta.updated_at.to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO thread_meta (thread_id, created_at, updated_at, label)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(thread_id) DO UPDATE SET
                updated_at = excluded.updated_at,
                label = excluded.label
            "#,
        )
        .bind(&id)
        .bind(&created)
        .bind(&updated)
        .bind(&meta.label)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_thread(&self, thread_id: Uuid) -> Result<ThreadMeta, StateError> {
        let id = thread_id.to_string();
        let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT thread_id, created_at, updated_at, label FROM thread_meta WHERE thread_id = ?1",
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let Some((tid, ca, ua, label)) = row else {
            return Err(StateError::NotFound(thread_id.to_string()));
        };
        let tid: Uuid =
            tid.parse().map_err(|e: uuid::Error| StateError::Backend(format!("uuid: {e}")))?;
        Ok(ThreadMeta {
            thread_id: tid,
            created_at: DateTime::parse_from_rfc3339(&ca)
                .map_err(|e| StateError::Backend(e.to_string()))?
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&ua)
                .map_err(|e| StateError::Backend(e.to_string()))?
                .with_timezone(&Utc),
            label,
        })
    }

    async fn delete_thread_meta(&self, thread_id: Uuid) -> Result<(), StateError> {
        let id = thread_id.to_string();
        let r = sqlx::query("DELETE FROM thread_meta WHERE thread_id = ?1")
            .bind(&id)
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
impl CheckpointStore for SqliteRuntimeStore {
    async fn save_checkpoint(&self, record: &CheckpointRecord) -> Result<(), StateError> {
        let payload = serde_json::to_string(record)
            .map_err(|e| StateError::Backend(format!("checkpoint encode: {e}")))?;
        let tid = record.thread_id.0.to_string();
        let rid = record.run_id.0.to_string();
        let step = record.step_seq.0 as i64;
        sqlx::query(
            r#"
            INSERT INTO checkpoints_step (thread_id, run_id, step_seq, payload)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(thread_id, run_id, step_seq) DO UPDATE SET payload = excluded.payload
            "#,
        )
        .bind(&tid)
        .bind(&rid)
        .bind(step)
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO checkpoints_latest (thread_id, run_id, payload)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(thread_id, run_id) DO UPDATE SET payload = excluded.payload
            "#,
        )
        .bind(&tid)
        .bind(&rid)
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
        let tid = thread_id.0.to_string();
        let rid = run_id.0.to_string();
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT payload FROM checkpoints_latest WHERE thread_id = ?1 AND run_id = ?2",
        )
        .bind(&tid)
        .bind(&rid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((payload,)) = row else {
            return Ok(None);
        };
        serde_json::from_str(&payload).map_err(|e| StateError::Backend(format!("checkpoint: {e}")))
    }

    async fn load_checkpoint_at_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        step_seq: StepSeq,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        let tid = thread_id.0.to_string();
        let rid = run_id.0.to_string();
        let step = step_seq.0 as i64;
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT payload FROM checkpoints_step WHERE thread_id = ?1 AND run_id = ?2 AND step_seq = ?3",
        )
        .bind(&tid)
        .bind(&rid)
        .bind(step)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((payload,)) = row else {
            return Ok(None);
        };
        serde_json::from_str(&payload).map_err(|e| StateError::Backend(format!("checkpoint: {e}")))
    }

    async fn list_checkpoint_steps_for_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> Result<Vec<StepSeq>, StateError> {
        let tid = thread_id.0.to_string();
        let rid = run_id.0.to_string();
        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT step_seq FROM checkpoints_step WHERE thread_id = ?1 AND run_id = ?2 ORDER BY step_seq ASC",
        )
        .bind(&tid)
        .bind(&rid)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(rows.into_iter().map(|(s,)| StepSeq(s as u64)).collect())
    }
}

#[async_trait]
impl ArtifactStore for SqliteRuntimeStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        let tid = thread_id.to_string();
        sqlx::query(
            r#"
            INSERT INTO artifacts (thread_id, name, bytes) VALUES (?1, ?2, ?3)
            ON CONFLICT(thread_id, name) DO UPDATE SET bytes = excluded.bytes
            "#,
        )
        .bind(&tid)
        .bind(name)
        .bind(bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(format!("sqlite:artifact:{tid}/{name}"))
    }

    async fn get_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StateError> {
        let tid = thread_id.to_string();
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT bytes FROM artifacts WHERE thread_id = ?1 AND name = ?2")
                .bind(&tid)
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(b,)| b))
    }
}

#[async_trait]
impl MemoryStore for SqliteRuntimeStore {
    async fn load_memory_document(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        let tid = thread_id.to_string();
        let row: Option<(String,)> =
            sqlx::query_as("SELECT facts_json FROM memory_facts WHERE thread_id = ?1")
                .bind(&tid)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((j,)) = row else {
            return Ok(MemoryDocument::default());
        };
        decode_memory_json_str(&j).map_err(StateError::Backend)
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        let tid = thread_id.to_string();
        if !doc.has_any_content() {
            sqlx::query("DELETE FROM memory_facts WHERE thread_id = ?1")
                .bind(&tid)
                .execute(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
            return Ok(());
        }
        let facts_json =
            serde_json::to_string(doc).map_err(|e| StateError::Backend(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO memory_facts (thread_id, facts_json) VALUES (?1, ?2)
            ON CONFLICT(thread_id) DO UPDATE SET facts_json = excluded.facts_json
            "#,
        )
        .bind(&tid)
        .bind(facts_json)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT thread_id, facts_json FROM memory_facts")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        for (s, j) in rows {
            let Ok(u) = Uuid::parse_str(&s) else {
                continue;
            };
            if decode_memory_json_str(&j).map_err(StateError::Backend)?.has_any_content() {
                out.push(u);
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl SkillStore for SqliteRuntimeStore {
    async fn list_skills(&self) -> Result<Vec<SkillRecord>, StateError> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT name, enabled FROM skills ORDER BY name")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(rows.into_iter().map(|(name, en)| SkillRecord { name, enabled: en != 0 }).collect())
    }

    async fn put_skill(&self, record: &SkillRecord) -> Result<(), StateError> {
        sqlx::query(
            r#"
            INSERT INTO skills (name, enabled) VALUES (?1, ?2)
            ON CONFLICT(name) DO UPDATE SET enabled = excluded.enabled
            "#,
        )
        .bind(&record.name)
        .bind(if record.enabled { 1i64 } else { 0i64 })
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_skill(&self, name: &str) -> Result<Option<SkillRecord>, StateError> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT name, enabled FROM skills WHERE name = ?1")
                .bind(name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(n, en)| SkillRecord { name: n, enabled: en != 0 }))
    }
}

#[async_trait]
impl ToolRecordStore for SqliteRuntimeStore {
    async fn append_tool_record(&self, record: &ToolRecord) -> Result<(), StateError> {
        let tid = record.thread_id.to_string();
        let record_json = serde_json::to_string(record)
            .map_err(|e| StateError::Backend(format!("tool record: {e}")))?;
        sqlx::query("INSERT INTO tool_records (thread_id, record_json) VALUES (?1, ?2)")
            .bind(&tid)
            .bind(record_json)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_tool_records(&self, thread_id: Uuid) -> Result<Vec<ToolRecord>, StateError> {
        let tid = thread_id.to_string();
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT record_json FROM tool_records WHERE thread_id = ?1 ORDER BY id")
                .bind(&tid)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (j,) in rows {
            let r: ToolRecord =
                serde_json::from_str(&j).map_err(|e| StateError::Backend(format!("tool: {e}")))?;
            out.push(r);
        }
        Ok(out)
    }
}

#[async_trait]
impl SubagentTaskStore for SqliteRuntimeStore {
    async fn upsert_task(&self, task: &SubagentTask) -> Result<(), StateError> {
        let record_json = serde_json::to_string(task)
            .map_err(|e| StateError::Backend(format!("subagent: {e}")))?;
        let tid = task.thread_id.to_string();
        let task_id = task.task_id.to_string();
        sqlx::query(
            r#"
            INSERT INTO subagent_tasks (task_id, thread_id, record_json) VALUES (?1, ?2, ?3)
            ON CONFLICT(task_id) DO UPDATE SET thread_id = excluded.thread_id, record_json = excluded.record_json
            "#,
        )
        .bind(&task_id)
        .bind(&tid)
        .bind(record_json)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: Uuid) -> Result<Option<SubagentTask>, StateError> {
        let id = task_id.to_string();
        let row: Option<(String,)> =
            sqlx::query_as("SELECT record_json FROM subagent_tasks WHERE task_id = ?1")
                .bind(&id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((j,)) = row else {
            return Ok(None);
        };
        serde_json::from_str(&j).map_err(|e| StateError::Backend(format!("subagent: {e}")))
    }

    async fn list_tasks_by_thread(&self, thread_id: Uuid) -> Result<Vec<SubagentTask>, StateError> {
        let tid = thread_id.to_string();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT record_json FROM subagent_tasks WHERE thread_id = ?1 ORDER BY task_id",
        )
        .bind(&tid)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (j,) in rows {
            let t: SubagentTask = serde_json::from_str(&j)
                .map_err(|e| StateError::Backend(format!("subagent: {e}")))?;
            out.push(t);
        }
        Ok(out)
    }
}

#[async_trait]
impl SandboxExecutionStore for SqliteRuntimeStore {
    async fn append_execution(&self, exec: &SandboxExecution) -> Result<(), StateError> {
        let tid = exec.thread_id.to_string();
        let record_json = serde_json::to_string(exec)
            .map_err(|e| StateError::Backend(format!("sandbox: {e}")))?;
        sqlx::query("INSERT INTO sandbox_executions (thread_id, record_json) VALUES (?1, ?2)")
            .bind(&tid)
            .bind(record_json)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_executions(&self, thread_id: Uuid) -> Result<Vec<SandboxExecution>, StateError> {
        let tid = thread_id.to_string();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT record_json FROM sandbox_executions WHERE thread_id = ?1 ORDER BY id",
        )
        .bind(&tid)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for (j,) in rows {
            let e: SandboxExecution = serde_json::from_str(&j)
                .map_err(|e| StateError::Backend(format!("sandbox: {e}")))?;
            out.push(e);
        }
        Ok(out)
    }
}

#[async_trait]
impl ManageTaskStore for SqliteRuntimeStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        Self::require_thread_id(&task.thread_id)?;
        let output_chunks = serde_json::to_string(&task.output_chunks)
            .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO manage_tasks (
                task_id, thread_id, status, output_chunks, error, callback_url, stream,
                client_task_id, tenant_id, user_id, created_at, updated_at, version
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT (task_id) DO UPDATE SET
                thread_id = excluded.thread_id,
                status = excluded.status,
                output_chunks = excluded.output_chunks,
                error = excluded.error,
                callback_url = excluded.callback_url,
                stream = excluded.stream,
                client_task_id = excluded.client_task_id,
                tenant_id = excluded.tenant_id,
                user_id = excluded.user_id,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                version = excluded.version
            "#,
        )
        .bind(&task.task_id)
        .bind(&task.thread_id)
        .bind(&task.status)
        .bind(output_chunks)
        .bind(&task.error)
        .bind(&task.callback_url)
        .bind(if task.stream { 1i64 } else { 0i64 })
        .bind(&task.client_task_id)
        .bind(&task.tenant_id)
        .bind(&task.user_id)
        .bind(task.created_at.to_rfc3339())
        .bind(task.updated_at.to_rfc3339())
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
            String,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
            String,
            String,
            String,
            String,
            i64,
        )> = sqlx::query_as(
            r#"
            SELECT task_id, thread_id, status, output_chunks, error, callback_url, stream,
                   client_task_id, tenant_id, user_id, created_at, updated_at, version
            FROM manage_tasks
            WHERE task_id = ?
            "#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let output_chunks: Vec<String> = serde_json::from_str(&row.3)
            .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
        let created_at = DateTime::parse_from_rfc3339(&row.10)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| StateError::Backend(e.to_string()))?;
        let updated_at = DateTime::parse_from_rfc3339(&row.11)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(Some(ManageTaskRecord {
            task_id: row.0,
            thread_id: row.1,
            status: row.2,
            output_chunks,
            error: row.4,
            callback_url: row.5,
            stream: row.6 != 0,
            client_task_id: row.7,
            tenant_id: row.8,
            user_id: row.9,
            created_at,
            updated_at,
            version: row.12,
        }))
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        Self::require_thread_id(thread_id)?;
        let rows: Vec<(
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
            String,
            String,
            String,
            String,
            i64,
        )> = sqlx::query_as(
            r#"
            SELECT task_id, thread_id, status, output_chunks, error, callback_url, stream,
                   client_task_id, tenant_id, user_id, created_at, updated_at, version
            FROM manage_tasks
            WHERE thread_id = ?
            ORDER BY updated_at DESC
            "#,
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let output_chunks: Vec<String> = serde_json::from_str(&row.3)
                .map_err(|e| StateError::Backend(format!("output_chunks: {e}")))?;
            let created_at = DateTime::parse_from_rfc3339(&row.10)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| StateError::Backend(e.to_string()))?;
            let updated_at = DateTime::parse_from_rfc3339(&row.11)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| StateError::Backend(e.to_string()))?;
            out.push(ManageTaskRecord {
                task_id: row.0,
                thread_id: row.1,
                status: row.2,
                output_chunks,
                error: row.4,
                callback_url: row.5,
                stream: row.6 != 0,
                client_task_id: row.7,
                tenant_id: row.8,
                user_id: row.9,
                created_at,
                updated_at,
                version: row.12,
            });
        }
        Ok(out)
    }
}

#[async_trait]
impl McpConfigStore for SqliteRuntimeStore {
    async fn get_mcp_servers(&self) -> Result<serde_json::Value, StateError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM app_kv WHERE key = 'mcp_servers'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((j,)) = row else {
            return Ok(serde_json::json!({}));
        };
        serde_json::from_str(&j).map_err(|e| StateError::Backend(format!("mcp: {e}")))
    }

    async fn put_mcp_servers(&self, value: &serde_json::Value) -> Result<(), StateError> {
        let j =
            serde_json::to_string(value).map_err(|e| StateError::Backend(format!("mcp: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO app_kv (key, value) VALUES ('mcp_servers', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(j)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadUploadStore for SqliteRuntimeStore {
    async fn list_upload_filenames(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let tid = thread_id.to_string();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT filename FROM thread_uploads WHERE thread_id = ?1 ORDER BY filename",
        )
        .bind(&tid)
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
        let tid = thread_id.to_string();
        sqlx::query(
            r#"
            INSERT INTO thread_uploads (thread_id, filename, bytes) VALUES (?1, ?2, ?3)
            ON CONFLICT(thread_id, filename) DO UPDATE SET bytes = excluded.bytes
            "#,
        )
        .bind(&tid)
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
        let tid = thread_id.to_string();
        let row: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT bytes FROM thread_uploads WHERE thread_id = ?1 AND filename = ?2",
        )
        .bind(&tid)
        .bind(filename)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(row.map(|(b,)| b))
    }

    async fn delete_upload(&self, thread_id: Uuid, filename: &str) -> Result<(), StateError> {
        let tid = thread_id.to_string();
        sqlx::query("DELETE FROM thread_uploads WHERE thread_id = ?1 AND filename = ?2")
            .bind(&tid)
            .bind(filename)
            .execute(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ManageConfigStore for SqliteRuntimeStore {
    async fn get_manage_app_config(&self) -> Result<ManageAppConfig, StateError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM app_kv WHERE key = 'manage_app'")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((j,)) = row else {
            return Ok(ManageAppConfig::default());
        };
        serde_json::from_str(&j).map_err(|e| StateError::Backend(format!("manage_app: {e}")))
    }

    async fn put_manage_app_config(&self, cfg: &ManageAppConfig) -> Result<(), StateError> {
        let j = serde_json::to_string(cfg)
            .map_err(|e| StateError::Backend(format!("manage_app: {e}")))?;
        sqlx::query(
            r#"
            INSERT INTO app_kv (key, value) VALUES ('manage_app', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(j)
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

impl SqliteRuntimeStore {
    async fn persist_lifecycle_op_sqlite(
        &self,
        report: &DeleteThreadReport,
    ) -> Result<(), StateError> {
        let json = serde_json::to_string(report).map_err(|e| StateError::Backend(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO thread_lifecycle_ops (op_id, thread_id, report_json, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(report.operation_id.to_string())
        .bind(report.thread_id.to_string())
        .bind(json)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ThreadLifecycleStore for SqliteRuntimeStore {
    async fn delete_thread_cascade_report(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteThreadReport, StateError> {
        let operation_id = Uuid::new_v4();
        let tid = thread_id.to_string();
        let consistency = DeleteConsistencyLevel::StrongPerThread;
        let mut tx = self.pool.begin().await.map_err(|e| StateError::Backend(e.to_string()))?;

        macro_rules! del {
            ($phase:expr, $q:literal) => {
                if let Err(e) = sqlx::query($q).bind(&tid).execute(&mut *tx).await {
                    let _ = tx.rollback().await;
                    let r = DeleteThreadReport {
                        operation_id,
                        thread_id,
                        status: DeleteThreadStatus::Failed { at: $phase, error: e.to_string() },
                        completed_phases: vec![],
                        consistency,
                        retryable: true,
                    };
                    let _ = self.persist_lifecycle_op_sqlite(&r).await;
                    return Ok(r);
                }
            };
        }

        del!(DeleteThreadPhase::Checkpoints, "DELETE FROM checkpoints_latest WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Checkpoints, "DELETE FROM checkpoints_step WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Memory, "DELETE FROM memory_facts WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Tools, "DELETE FROM tool_records WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Subagents, "DELETE FROM subagent_tasks WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Sandbox, "DELETE FROM sandbox_executions WHERE thread_id = ?1");
        del!(DeleteThreadPhase::ManageTasks, "DELETE FROM manage_tasks WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Artifacts, "DELETE FROM artifacts WHERE thread_id = ?1");
        del!(DeleteThreadPhase::Uploads, "DELETE FROM thread_uploads WHERE thread_id = ?1");
        del!(DeleteThreadPhase::ThreadMeta, "DELETE FROM thread_meta WHERE thread_id = ?1");

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
        let json = match serde_json::to_string(&report) {
            Ok(j) => j,
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(StateError::Backend(e.to_string()));
            }
        };
        sqlx::query(
            r#"
            INSERT INTO thread_lifecycle_ops (op_id, thread_id, report_json, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(operation_id.to_string())
        .bind(&tid)
        .bind(json)
        .bind(Utc::now().to_rfc3339())
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
        let tid = thread_id.to_string();
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT report_json FROM thread_lifecycle_ops WHERE thread_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&tid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| StateError::Backend(e.to_string()))?;
        let Some((json,)) = row else {
            return Ok(None);
        };
        serde_json::from_str(&json).map_err(|e| StateError::Backend(format!("report json: {e}")))
    }

    async fn verify_thread_deletion(
        &self,
        thread_id: Uuid,
    ) -> Result<DeleteVerifyReport, StateError> {
        let tid = thread_id.to_string();
        let mut residual_by_phase = HashMap::new();
        let c: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM checkpoints_latest WHERE thread_id = ?1")
                .bind(&tid)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        let c2: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM checkpoints_step WHERE thread_id = ?1")
                .bind(&tid)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Checkpoints, c.0 + c2.0 > 0);
        let m: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_facts WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Memory, m.0 > 0);
        let t: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tool_records WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Tools, t.0 > 0);
        let s: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM subagent_tasks WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Subagents, s.0 > 0);
        let sb: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sandbox_executions WHERE thread_id = ?1")
                .bind(&tid)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Sandbox, sb.0 > 0);
        let mt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM manage_tasks WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::ManageTasks, mt.0 > 0);
        let a: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM artifacts WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Artifacts, a.0 > 0);
        let u: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM thread_uploads WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::Uploads, u.0 > 0);
        let tm: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM thread_meta WHERE thread_id = ?1")
            .bind(&tid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        residual_by_phase.insert(DeleteThreadPhase::ThreadMeta, tm.0 > 0);
        Ok(DeleteVerifyReport { thread_id, residual_by_phase })
    }
}
