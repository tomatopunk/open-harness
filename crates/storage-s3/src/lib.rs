//! S3-compatible object storage for artifacts.

mod artifact;
mod manage_task;

pub use artifact::S3ArtifactStore;
pub use manage_task::{build_s3_manage_store, S3ManageTaskStore};
