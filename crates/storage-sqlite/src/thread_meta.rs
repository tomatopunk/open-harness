use async_trait::async_trait;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use state_abstraction::{StateError, ThreadMeta, ThreadMetaStore};
use uuid::Uuid;

pub struct SqliteThreadMetaStore {
    pool: SqlitePool,
}

impl SqliteThreadMetaStore {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(url).await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS thread_meta (
                thread_id TEXT PRIMARY KEY,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                label TEXT
            )
            "#,
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl ThreadMetaStore for SqliteThreadMetaStore {
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
        let tid: Uuid = tid
            .parse()
            .map_err(|e: uuid::Error| StateError::Backend(format!("uuid parse: {e}")))?;
        Ok(ThreadMeta {
            thread_id: tid,
            created_at: chrono::DateTime::parse_from_rfc3339(&ca)
                .map_err(|e| StateError::Backend(e.to_string()))?
                .with_timezone(&Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(&ua)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip() {
        let url = "sqlite::memory:";
        let store = SqliteThreadMetaStore::connect(url).await.unwrap();
        let id = Uuid::new_v4();
        let now = Utc::now();
        let meta =
            ThreadMeta { thread_id: id, created_at: now, updated_at: now, label: Some("x".into()) };
        store.upsert_thread(&meta).await.unwrap();
        let got = store.get_thread(id).await.unwrap();
        assert_eq!(got.thread_id, id);
    }
}
