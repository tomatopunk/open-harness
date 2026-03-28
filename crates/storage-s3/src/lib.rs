//! S3-compatible object storage for artifacts.

mod artifact;
mod manage_task;
mod s3_runtime;

use state_abstraction::StateError;

pub use artifact::S3ArtifactStore;
pub use manage_task::{build_s3_manage_store, S3ManageTaskStore};
pub use s3_runtime::S3RuntimeStore;

/// Build unified S3 runtime store (same credential chain as `build_s3_manage_store`).
pub fn build_s3_runtime_store(
    bucket: &str,
    prefix: impl Into<String>,
) -> Result<S3RuntimeStore, StateError> {
    use object_store::aws::AmazonS3Builder;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
    let inner = AmazonS3Builder::new()
        .with_bucket_name(bucket)
        .with_region(region)
        .build()
        .map_err(|e| StateError::Backend(e.to_string()))?;
    Ok(S3RuntimeStore::new(
        std::sync::Arc::new(inner) as std::sync::Arc<dyn object_store::ObjectStore>,
        prefix,
    ))
}
