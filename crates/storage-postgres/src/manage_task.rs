use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use state_abstraction::{ManageTaskRecord, ManageTaskStore, StateError};

pub struct PostgresManageTaskStore {
    pool: PgPool,
}

impl PostgresManageTaskStore {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        sqlx::query(
            r#"
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
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl ManageTaskStore for PostgresManageTaskStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        let output_chunks = serde_json::to_value(&task.output_chunks)
            .map_err(|e| StateError::Backend(format!("encode output_chunks: {e}")))?;
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
        let output_chunks = serde_json::from_value::<Vec<String>>(row.3)
            .map_err(|e| StateError::Backend(format!("decode output_chunks: {e}")))?;
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
            let output_chunks = serde_json::from_value::<Vec<String>>(row.3)
                .map_err(|e| StateError::Backend(format!("decode output_chunks: {e}")))?;
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
