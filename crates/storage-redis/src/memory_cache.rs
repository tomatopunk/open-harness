use async_trait::async_trait;
use redis::AsyncCommands;
use state_abstraction::{
    memory_document::{
        decode_memory_json_str, Fact, MemoryDocument, MemoryHistory, MemoryUserProfile,
    },
    MemoryStore, StateError,
};
use uuid::Uuid;

pub struct RedisMemoryStore {
    client: redis::Client,
}

impl RedisMemoryStore {
    pub fn new(url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self { client: redis::Client::open(url)? })
    }

    fn key(thread_id: Uuid) -> String {
        format!("open-harness:memory:{thread_id}")
    }
}

#[async_trait]
impl MemoryStore for RedisMemoryStore {
    async fn load_memory_document(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        let k = Self::key(thread_id);
        let t: Option<String> = redis::cmd("TYPE")
            .arg(&k)
            .query_async(&mut conn)
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        match t.as_deref() {
            Some("string") => {
                let raw: Option<String> =
                    conn.get(&k).await.map_err(|e| StateError::Backend(e.to_string()))?;
                let Some(raw) = raw else {
                    return Ok(MemoryDocument::default());
                };
                decode_memory_json_str(&raw).map_err(StateError::Backend)
            }
            Some("list") => {
                let facts: Vec<String> =
                    conn.lrange(&k, 0, -1).await.map_err(|e| StateError::Backend(e.to_string()))?;
                Ok(MemoryDocument {
                    schema_version: state_abstraction::MEMORY_DOCUMENT_SCHEMA_VERSION,
                    facts: facts
                        .into_iter()
                        .map(|s| {
                            Fact::new(
                                s,
                                state_abstraction::memory_document::FactCategory::Knowledge,
                                1.0,
                                "unknown".to_string(),
                            )
                        })
                        .collect(),
                    user: MemoryUserProfile::default(),
                    history: MemoryHistory::default(),
                    metadata: state_abstraction::memory_document::MemoryMetadata::default(),
                })
            }
            _ => Ok(MemoryDocument::default()),
        }
    }

    async fn save_memory_document(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        let k = Self::key(thread_id);
        let _: () = conn.del(&k).await.map_err(|e| StateError::Backend(e.to_string()))?;
        if !doc.has_any_content() {
            return Ok(());
        }
        let payload =
            serde_json::to_string(doc).map_err(|e| StateError::Backend(format!("memory: {e}")))?;
        let _: () = conn.set(&k, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
        Ok(Vec::new())
    }
}
