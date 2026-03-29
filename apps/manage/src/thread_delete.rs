//! Thread delete: durable state via [`ThreadLifecycleStore::delete_thread_cascade`] on the active
//! `StorageRegistry` backend, then best-effort LangGraph HTTP `DELETE /threads/{id}` — decoupled with
//! compensation. (Legacy name `DeletingLocal` means “local deployment storage”, not `threads_root` FS paths.)

use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use state_abstraction::StateError;
use std::sync::Arc;
use std::time::Duration;
use storage_registry::RuntimeStorageBundle;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletePhase {
    Requested,
    /// Cascade delete on the configured storage backend (`delete_thread_cascade`).
    DeletingLocal,
    /// Cascade step finished (may be partial; see op errors).
    LocalDeleted,
    DeletingRemote,
    RemoteDeleted,
    Completed,
    CompletedWithWarning,
    Failed,
    PartialSuccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadDeleteOp {
    pub operation_id: Uuid,
    pub thread_id: Uuid,
    pub phase: DeletePhase,
    pub local_error: Option<String>,
    pub remote_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ThreadDeleteEngine {
    pub runtime: Arc<RwLock<RuntimeStorageBundle>>,
    pub langgraph_url: Arc<RwLock<String>>,
    pub http: reqwest::Client,
    pub ops: DashMap<Uuid, ThreadDeleteOp>,
    pub idempotency: DashMap<Uuid, (Uuid, DateTime<Utc>)>, // thread_id -> (operation_id, touched_at)
    op_retention: Duration,
    idempotency_retention: Duration,
    queue: Mutex<()>,
}

impl ThreadDeleteEngine {
    pub fn new(
        runtime: Arc<RwLock<RuntimeStorageBundle>>,
        langgraph_url: Arc<RwLock<String>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            langgraph_url,
            http: reqwest::Client::new(),
            ops: DashMap::new(),
            idempotency: DashMap::new(),
            op_retention: Duration::from_secs(30 * 60),
            idempotency_retention: Duration::from_secs(5 * 60),
            queue: Mutex::new(()),
        })
    }

    pub fn get_op(&self, op_id: Uuid) -> Option<ThreadDeleteOp> {
        self.ops.get(&op_id).map(|e| e.clone())
    }

    /// Start or return existing delete operation for `thread_id` (idempotent).
    pub fn start_delete(self: &Arc<Self>, thread_id: Uuid) -> Uuid {
        self.compact_state();
        match self.idempotency.entry(thread_id) {
            Entry::Occupied(mut existing) => {
                let operation_id = existing.get().0;
                existing.insert((operation_id, Utc::now()));
                operation_id
            }
            Entry::Vacant(slot) => {
                let op_id = Uuid::new_v4();
                let now = Utc::now();
                let op = ThreadDeleteOp {
                    operation_id: op_id,
                    thread_id,
                    phase: DeletePhase::Requested,
                    local_error: None,
                    remote_error: None,
                    created_at: now,
                    updated_at: now,
                };
                self.ops.insert(op_id, op);
                slot.insert((op_id, now));
                let this = Arc::clone(self);
                tokio::spawn(async move {
                    this.run_delete(op_id, thread_id).await;
                });
                op_id
            }
        }
    }

    async fn run_delete(&self, op_id: Uuid, thread_id: Uuid) {
        let _guard = self.queue.lock().await;
        self.update_phase(op_id, DeletePhase::DeletingLocal);

        let local_res = {
            let rt = self.runtime.read().await;
            rt.registry.lifecycle.delete_thread_cascade(thread_id).await
        };
        match local_res {
            Ok(()) => {
                self.update_phase(op_id, DeletePhase::LocalDeleted);
            }
            Err(StateError::LifecycleIncomplete(report)) => {
                let msg = report.summary();
                self.patch_op(op_id, |o| {
                    o.local_error = Some(msg);
                    o.phase = DeletePhase::PartialSuccess;
                });
                self.update_phase(op_id, DeletePhase::LocalDeleted);
            }
            Err(e) => {
                let msg = e.to_string();
                self.patch_op(op_id, |o| {
                    o.local_error = Some(msg.clone());
                    o.phase = DeletePhase::Failed;
                });
                return;
            }
        }

        self.update_phase(op_id, DeletePhase::DeletingRemote);
        let langgraph_url = self.langgraph_url.read().await.clone();
        let url = format!("{}/threads/{}", langgraph_url.trim_end_matches('/'), thread_id);
        match self.http.delete(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() || status.as_u16() == 404 {
                    self.update_phase(op_id, DeletePhase::RemoteDeleted);
                    self.update_phase(op_id, DeletePhase::Completed);
                } else {
                    let msg = format!("http {}", status);
                    self.patch_op(op_id, |o| {
                        o.remote_error = Some(msg);
                        o.phase = DeletePhase::PartialSuccess;
                    });
                    self.update_phase(op_id, DeletePhase::CompletedWithWarning);
                }
            }
            Err(e) => {
                let msg = e.to_string();
                self.patch_op(op_id, |o| {
                    o.remote_error = Some(msg);
                    o.phase = DeletePhase::PartialSuccess;
                });
                self.update_phase(op_id, DeletePhase::CompletedWithWarning);
            }
        }

        self.compact_state();
    }

    fn update_phase(&self, op_id: Uuid, phase: DeletePhase) {
        self.patch_op(op_id, |o| {
            o.phase = phase;
            o.updated_at = Utc::now();
        });
    }

    fn patch_op(&self, op_id: Uuid, f: impl FnOnce(&mut ThreadDeleteOp)) {
        if let Some(mut e) = self.ops.get_mut(&op_id) {
            f(&mut e);
        }
    }

    fn compact_state(&self) {
        let now = Utc::now();
        let op_retention = chrono::Duration::from_std(self.op_retention)
            .unwrap_or_else(|_| chrono::Duration::minutes(30));
        let idempotency_retention = chrono::Duration::from_std(self.idempotency_retention)
            .unwrap_or_else(|_| chrono::Duration::minutes(5));

        self.ops.retain(|_, op| now.signed_duration_since(op.updated_at) <= op_retention);
        self.idempotency.retain(|_, (_, touched_at)| {
            now.signed_duration_since(*touched_at) <= idempotency_retention
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use state_abstraction::{
        ArtifactStore, CheckpointStore, LocalFsStateStore, ManageConfigStore, ManageTaskStore,
        McpConfigStore, MemoryStore, SandboxExecutionStore, SkillStore, StorageRegistry,
        SubagentTaskStore, ThreadLifecycleStore, ThreadMetaStore, ThreadUploadStore,
        ToolRecordStore,
    };
    use std::collections::HashSet;

    fn test_bundle(root: std::path::PathBuf) -> RuntimeStorageBundle {
        let store = Arc::new(LocalFsStateStore::new(root));
        let registry = StorageRegistry::new(
            store.clone() as Arc<dyn ThreadMetaStore>,
            store.clone() as Arc<dyn CheckpointStore>,
            store.clone() as Arc<dyn ArtifactStore>,
            store.clone() as Arc<dyn ThreadUploadStore>,
            store.clone() as Arc<dyn MemoryStore>,
            store.clone() as Arc<dyn SkillStore>,
            store.clone() as Arc<dyn ToolRecordStore>,
            store.clone() as Arc<dyn SubagentTaskStore>,
            store.clone() as Arc<dyn SandboxExecutionStore>,
            store.clone() as Arc<dyn ManageTaskStore>,
            store.clone() as Arc<dyn McpConfigStore>,
            store.clone() as Arc<dyn ManageConfigStore>,
            store.clone() as Arc<dyn ThreadLifecycleStore>,
        );
        RuntimeStorageBundle { registry, capabilities: json!({}), active_mode: "local_fs".into() }
    }

    #[tokio::test]
    async fn local_delete_idempotent_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let eng = ThreadDeleteEngine::new(
            Arc::new(RwLock::new(test_bundle(tmp.path().to_path_buf()))),
            Arc::new(RwLock::new("http://127.0.0.1:9".into())),
        );
        let tid = Uuid::new_v4();
        let op = eng.start_delete(tid);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let o = eng.get_op(op).expect("op");
        assert!(
            matches!(
                o.phase,
                DeletePhase::Completed | DeletePhase::CompletedWithWarning | DeletePhase::Failed
            ),
            "phase {:?}",
            o.phase
        );
    }

    #[tokio::test]
    async fn concurrent_start_delete_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let eng = ThreadDeleteEngine::new(
            Arc::new(RwLock::new(test_bundle(tmp.path().to_path_buf()))),
            Arc::new(RwLock::new("http://127.0.0.1:9".into())),
        );
        let tid = Uuid::new_v4();

        let mut handles = Vec::new();
        for _ in 0..32 {
            let eng = Arc::clone(&eng);
            handles.push(tokio::spawn(async move { eng.start_delete(tid) }));
        }

        let mut op_ids = HashSet::new();
        for handle in handles {
            let op_id = handle.await.expect("join");
            op_ids.insert(op_id);
        }

        assert_eq!(op_ids.len(), 1, "same thread should share one operation");
        assert_eq!(eng.idempotency.len(), 1);
        assert_eq!(eng.ops.len(), 1);
    }
}
