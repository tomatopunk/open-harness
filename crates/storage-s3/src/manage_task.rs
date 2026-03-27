//! S3-backed manage task store using JSON objects and per-thread index files.

use async_trait::async_trait;
use bytes::Bytes;
use object_store::Error as ObjectStoreError;
use object_store::{path::Path, ObjectStore};
use state_abstraction::{ManageTaskRecord, ManageTaskStore, StateError};
use std::sync::Arc;

/// Build an S3-backed store using default AWS credential chain and `AWS_REGION` (default `us-east-1`).
pub fn build_s3_manage_store(
    bucket: &str,
    prefix: impl Into<String>,
) -> Result<S3ManageTaskStore, StateError> {
    use object_store::aws::AmazonS3Builder;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
    let inner = AmazonS3Builder::new()
        .with_bucket_name(bucket)
        .with_region(region)
        .build()
        .map_err(|e| StateError::Backend(e.to_string()))?;
    Ok(S3ManageTaskStore::new(Arc::new(inner) as Arc<dyn ObjectStore>, prefix))
}

pub struct S3ManageTaskStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl S3ManageTaskStore {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: impl Into<String>) -> Self {
        Self { store, prefix: prefix.into().trim_matches('/').to_string() }
    }

    fn task_path(&self, task_id: &str) -> Path {
        Path::from(format!("{}/manage_tasks/v1/tasks/{}.json", self.prefix, task_id))
    }

    fn thread_index_path(&self, thread_id: &str) -> Path {
        Path::from(format!("{}/manage_tasks/v1/thread_index/{}.json", self.prefix, thread_id))
    }

    async fn read_thread_ids(&self, thread_id: &str) -> Result<Vec<String>, StateError> {
        let path = self.thread_index_path(thread_id);
        let bytes = match self.store.get(&path).await {
            Ok(g) => g.bytes().await.map_err(|e| StateError::Backend(e.to_string()))?,
            Err(ObjectStoreError::NotFound { .. }) => return Ok(Vec::new()),
            Err(e) => return Err(StateError::Backend(e.to_string())),
        };
        let s = String::from_utf8(bytes.to_vec())
            .map_err(|e| StateError::Backend(format!("utf8: {e}")))?;
        let ids: Vec<String> = serde_json::from_str(&s).unwrap_or_else(|_| Vec::new());
        Ok(ids)
    }

    async fn write_thread_ids(&self, thread_id: &str, ids: &[String]) -> Result<(), StateError> {
        let path = self.thread_index_path(thread_id);
        let payload = serde_json::to_vec(ids).map_err(|e| StateError::Backend(e.to_string()))?;
        self.store
            .put(&path, Bytes::from(payload).into())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl ManageTaskStore for S3ManageTaskStore {
    async fn upsert_task(&self, task: &ManageTaskRecord) -> Result<(), StateError> {
        let path = self.task_path(&task.task_id);
        let payload = serde_json::to_vec(task).map_err(|e| StateError::Backend(e.to_string()))?;
        self.store
            .put(&path, Bytes::from(payload).into())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;

        let mut ids = self.read_thread_ids(&task.thread_id).await?;
        if !ids.iter().any(|x| x == &task.task_id) {
            ids.push(task.task_id.clone());
        }
        self.write_thread_ids(&task.thread_id, &ids).await?;
        Ok(())
    }

    async fn get_task(&self, task_id: &str) -> Result<Option<ManageTaskRecord>, StateError> {
        let path = self.task_path(task_id);
        let bytes = match self.store.get(&path).await {
            Ok(g) => g.bytes().await.map_err(|e| StateError::Backend(e.to_string()))?,
            Err(ObjectStoreError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(StateError::Backend(e.to_string())),
        };
        let task: ManageTaskRecord = serde_json::from_slice(&bytes)
            .map_err(|e| StateError::Backend(format!("decode task: {e}")))?;
        Ok(Some(task))
    }

    async fn list_tasks_by_thread(
        &self,
        thread_id: &str,
    ) -> Result<Vec<ManageTaskRecord>, StateError> {
        let ids = self.read_thread_ids(thread_id).await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(t) = self.get_task(&id).await? {
                out.push(t);
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }
}
