//! Build a [`StorageRegistry`] from [`config_runtime::AppConfig`] for all supported backends.

use std::path::PathBuf;
use std::sync::Arc;

use config_runtime::AppConfig;
use serde_json::json;
use state_abstraction::{LocalFsLayout, LocalFsStateStore, StorageRegistry};
use thiserror::Error;

/// Bundle returned by [`build_runtime_storage`].
#[derive(Clone)]
pub struct RuntimeStorageBundle {
    pub registry: StorageRegistry,
    pub capabilities: serde_json::Value,
    pub active_mode: String,
}

/// Errors when constructing storage from configuration.
#[derive(Debug, Error)]
pub enum StorageBuildError {
    #[error("missing configuration: {0}")]
    MissingConfig(String),
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("state backend: {0}")]
    State(#[from] state_abstraction::StateError),
}

fn sqlite_connection_url(cfg: &AppConfig) -> Option<String> {
    let u = cfg.storage.sqlite.url.clone().or(cfg.manage.sqlite_url.clone())?;
    Some(if u.starts_with("sqlite:") {
        u
    } else {
        format!("sqlite://{}", u.trim_start_matches('/'))
    })
}

fn capabilities_all_supported() -> serde_json::Value {
    json!({
        "manage_tasks": true,
        "memory": true,
        "skills": true,
        "tool_records": true,
        "subagent_tasks": true,
        "sandbox_logs": true,
        "checkpoints": true,
        "artifacts": true,
        "thread_uploads": true,
        "thread_meta": true,
        "mcp_config": true,
        "manage_config": true,
        "thread_lifecycle": true,
    })
}

/// Construct a full [`StorageRegistry`] for `cfg.storage.mode` (hard-selected backend; no fallback).
pub async fn build_runtime_storage(
    cfg: &AppConfig,
) -> Result<RuntimeStorageBundle, StorageBuildError> {
    let mode = cfg.storage.mode.as_str();
    match mode {
        "local_fs" => {
            let root = PathBuf::from(&cfg.storage.local_fs.root);
            let layout = LocalFsLayout::new(&root);
            let _ = layout.ensure_base_dirs();
            let store = Arc::new(LocalFsStateStore::new(root));
            let registry = StorageRegistry::new(
                store.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
                store.clone() as Arc<dyn state_abstraction::CheckpointStore>,
                store.clone() as Arc<dyn state_abstraction::ArtifactStore>,
                store.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
                store.clone() as Arc<dyn state_abstraction::MemoryStore>,
                store.clone() as Arc<dyn state_abstraction::SkillStore>,
                store.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
                store.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
                store.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
                store.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
                store.clone() as Arc<dyn state_abstraction::McpConfigStore>,
                store.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
                store.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
            );
            Ok(RuntimeStorageBundle {
                registry,
                capabilities: capabilities_all_supported(),
                active_mode: "local_fs".to_string(),
            })
        }
        "sqlite" => {
            let url = sqlite_connection_url(cfg).ok_or_else(|| {
                StorageBuildError::MissingConfig(
                    "sqlite_url (storage or manage) is required".into(),
                )
            })?;
            let store = storage_sqlite::SqliteRuntimeStore::connect(&url).await?;
            let arc = Arc::new(store);
            let registry = StorageRegistry::new(
                arc.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
                arc.clone() as Arc<dyn state_abstraction::CheckpointStore>,
                arc.clone() as Arc<dyn state_abstraction::ArtifactStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
                arc.clone() as Arc<dyn state_abstraction::MemoryStore>,
                arc.clone() as Arc<dyn state_abstraction::SkillStore>,
                arc.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
                arc.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::McpConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
            );
            Ok(RuntimeStorageBundle {
                registry,
                capabilities: capabilities_all_supported(),
                active_mode: "sqlite".to_string(),
            })
        }
        "postgres" => {
            let url =
                cfg.storage.postgres.url.clone().or(cfg.manage.postgres_url.clone()).ok_or_else(
                    || StorageBuildError::MissingConfig("postgres_url is required".into()),
                )?;
            let store = storage_postgres::PostgresRuntimeStore::connect(&url).await?;
            let arc = Arc::new(store);
            let registry = StorageRegistry::new(
                arc.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
                arc.clone() as Arc<dyn state_abstraction::CheckpointStore>,
                arc.clone() as Arc<dyn state_abstraction::ArtifactStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
                arc.clone() as Arc<dyn state_abstraction::MemoryStore>,
                arc.clone() as Arc<dyn state_abstraction::SkillStore>,
                arc.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
                arc.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::McpConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
            );
            Ok(RuntimeStorageBundle {
                registry,
                capabilities: capabilities_all_supported(),
                active_mode: "postgres".to_string(),
            })
        }
        "redis" => {
            let url =
                cfg.storage.redis.url.clone().ok_or_else(|| {
                    StorageBuildError::MissingConfig("redis_url is required".into())
                })?;
            let store = storage_redis::RedisRuntimeStore::connect(&url).await?;
            let arc = Arc::new(store);
            let registry = StorageRegistry::new(
                arc.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
                arc.clone() as Arc<dyn state_abstraction::CheckpointStore>,
                arc.clone() as Arc<dyn state_abstraction::ArtifactStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
                arc.clone() as Arc<dyn state_abstraction::MemoryStore>,
                arc.clone() as Arc<dyn state_abstraction::SkillStore>,
                arc.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
                arc.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::McpConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
            );
            Ok(RuntimeStorageBundle {
                registry,
                capabilities: capabilities_all_supported(),
                active_mode: "redis".to_string(),
            })
        }
        "s3" => {
            let bucket =
                cfg.storage.s3.bucket.clone().ok_or_else(|| {
                    StorageBuildError::MissingConfig("s3_bucket is required".into())
                })?;
            let prefix = cfg.storage.s3.prefix.clone();
            let store = storage_s3::build_s3_runtime_store(&bucket, prefix)?;
            let arc = Arc::new(store);
            let registry = StorageRegistry::new(
                arc.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
                arc.clone() as Arc<dyn state_abstraction::CheckpointStore>,
                arc.clone() as Arc<dyn state_abstraction::ArtifactStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
                arc.clone() as Arc<dyn state_abstraction::MemoryStore>,
                arc.clone() as Arc<dyn state_abstraction::SkillStore>,
                arc.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
                arc.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
                arc.clone() as Arc<dyn state_abstraction::McpConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
                arc.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
            );
            Ok(RuntimeStorageBundle {
                registry,
                capabilities: capabilities_all_supported(),
                active_mode: "s3".to_string(),
            })
        }
        _ => Err(StorageBuildError::MissingConfig(format!(
            "unknown storage.mode: {mode} (expected local_fs|sqlite|postgres|redis|s3)"
        ))),
    }
}
