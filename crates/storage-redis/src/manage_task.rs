//! Redis-backed manage task store.

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use state_abstraction::{ManageTaskRecord, ManageTaskStore, StateError};

const PREFIX_TASK: &str = "open_harness:mt:task:";
const PREFIX_THREAD: &str = "open_harness:mt:thread:";

pub struct RedisManageTaskStore {
    conn: ConnectionManager,
}

impl RedisManageTaskStore {
    pub async fn connect(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }
}

fn task_key(task_id: &str) -> String {
    format!("{PREFIX_TASK}{task_id}")
}

fn thread_key(thread_id: &str) -> String {
    format!("{PREFIX_THREAD}{thread_id}")
}

#[async_trait]
impl ManageTaskStore for RedisManageTaskStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        let payload = serde_json::to_string(task)
            .map_err(|e| StateError::Backend(format!("encode task: {e}")))?;
        let mut conn = self.conn.clone();
        let tk = task_key(&task.task_id);
        let _: () = conn.set(&tk, payload).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let th = thread_key(&task.thread_id);
        let _: () =
            conn.sadd(&th, &task.task_id).await.map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let mut conn = self.conn.clone();
        let tk = task_key(task_id);
        let raw: Option<String> =
            conn.get(tk).await.map_err(|e| StateError::Backend(e.to_string()))?;
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
        let mut conn = self.conn.clone();
        let th = thread_key(thread_id);
        let ids: Vec<String> =
            conn.smembers(th).await.map_err(|e| StateError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(t) = self.get_task(&id).await? {
                out.push(t);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }
}
