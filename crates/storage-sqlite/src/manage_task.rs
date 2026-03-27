//! SQLite-backed manage task store (parity with postgres manage_tasks table).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
use state_abstraction::{ManageTaskRecord, ManageTaskStore, StateError};

pub struct SqliteManageTaskStore {
    pool: SqlitePool,
}

impl SqliteManageTaskStore {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        sqlx::query(
            r#"
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
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl ManageTaskStore for SqliteManageTaskStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        let output_chunks = serde_json::to_string(&task.output_chunks)
            .map_err(|e| StateError::Backend(format!("encode output_chunks: {e}")))?;
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
            .map_err(|e| StateError::Backend(format!("decode output_chunks: {e}")))?;
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
                .map_err(|e| StateError::Backend(format!("decode output_chunks: {e}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use state_abstraction::ManageTaskRecord;

    #[tokio::test]
    async fn manage_task_roundtrip() {
        let store = SqliteManageTaskStore::connect("sqlite::memory:").await.expect("connect");
        let now = Utc::now();
        let rec = ManageTaskRecord {
            task_id: "t1".into(),
            thread_id: "th1".into(),
            status: "pending".into(),
            output_chunks: vec!["a".into()],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "default".into(),
            user_id: "u1".into(),
            created_at: now,
            updated_at: now,
            version: 1,
        };
        store.upsert_task(&rec).await.expect("upsert");
        let got = store.get_task("t1").await.expect("get").expect("some");
        assert_eq!(got.task_id, "t1");
        let list = store.list_tasks_by_thread("th1").await.expect("list");
        assert_eq!(list.len(), 1);
    }
}
