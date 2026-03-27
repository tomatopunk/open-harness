use async_trait::async_trait;
use chrono::Utc;
use sqlx::postgres::PgPool;
use state_abstraction::{StateError, ThreadMeta, ThreadMetaStore};
use uuid::Uuid;

pub struct PostgresThreadMetaStore {
    pool: PgPool,
}

impl PostgresThreadMetaStore {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS thread_meta (
                thread_id UUID PRIMARY KEY,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                label TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }
}

#[async_trait]
impl ThreadMetaStore for PostgresThreadMetaStore {
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
        let row: Option<(
            Uuid,
            chrono::DateTime<Utc>,
            chrono::DateTime<Utc>,
            Option<String>,
        )> = sqlx::query_as(
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
