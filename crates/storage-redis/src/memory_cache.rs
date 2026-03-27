use async_trait::async_trait;
use redis::AsyncCommands;
use state_abstraction::{MemoryStore, StateError};
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
    async fn append_fact(&self, thread_id: Uuid, fact: &str) -> Result<(), StateError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        let k = Self::key(thread_id);
        conn.rpush::<_, _, ()>(&k, fact).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_facts(&self, thread_id: Uuid) -> Result<Vec<String>, StateError> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        let k = Self::key(thread_id);
        let v: Vec<String> =
            conn.lrange(&k, 0, -1).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(v)
    }
}
