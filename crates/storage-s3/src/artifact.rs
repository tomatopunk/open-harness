use async_trait::async_trait;
use bytes::Bytes;
use object_store::{path::Path, ObjectStore};
use state_abstraction::{ArtifactStore, StateError};
use std::sync::Arc;
use uuid::Uuid;

pub struct S3ArtifactStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl S3ArtifactStore {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: impl Into<String>) -> Self {
        Self { store, prefix: prefix.into() }
    }
}

#[async_trait]
impl ArtifactStore for S3ArtifactStore {
    async fn put_artifact(
        &self,
        thread_id: Uuid,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, StateError> {
        let key = format!("{}/{}/{}", self.prefix.trim_end_matches('/'), thread_id, name);
        let path = Path::from(key.clone());
        self.store
            .put(&path, Bytes::copy_from_slice(bytes).into())
            .await
            .map_err(|e| StateError::Backend(e.to_string()))?;
        Ok(key)
    }
}
