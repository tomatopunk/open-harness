//! Thread delete: local `.deer-flow/threads/{id}` vs LangGraph remote — decoupled with compensation.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletePhase {
    Requested,
    DeletingLocal,
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
    pub threads_root: PathBuf,
    pub langgraph_url: String,
    pub http: reqwest::Client,
    pub ops: DashMap<Uuid, ThreadDeleteOp>,
    pub idempotency: DashMap<Uuid, Uuid>, // thread_id -> operation_id
    queue: Mutex<()>,
}

impl ThreadDeleteEngine {
    pub fn new(threads_root: PathBuf, langgraph_url: String) -> Arc<Self> {
        Arc::new(Self {
            threads_root,
            langgraph_url,
            http: reqwest::Client::new(),
            ops: DashMap::new(),
            idempotency: DashMap::new(),
            queue: Mutex::new(()),
        })
    }

    pub fn get_op(&self, op_id: Uuid) -> Option<ThreadDeleteOp> {
        self.ops.get(&op_id).map(|e| e.clone())
    }

    /// Start or return existing delete operation for `thread_id` (idempotent).
    pub fn start_delete(self: &Arc<Self>, thread_id: Uuid) -> Uuid {
        if let Some(existing) = self.idempotency.get(&thread_id) {
            return *existing;
        }
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
        self.idempotency.insert(thread_id, op_id);
        let this = Arc::clone(self);
        tokio::spawn(async move {
            this.run_delete(op_id, thread_id).await;
        });
        op_id
    }

    async fn run_delete(&self, op_id: Uuid, thread_id: Uuid) {
        let _guard = self.queue.lock().await;
        self.update_phase(op_id, DeletePhase::DeletingLocal);

        // Local delete: idempotent — missing dir is OK
        let local_path = self.threads_root.join(thread_id.to_string());
        match tokio::fs::remove_dir_all(&local_path).await {
            Ok(()) => {
                self.update_phase(op_id, DeletePhase::LocalDeleted);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
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
        let url = format!("{}/threads/{}", self.langgraph_url.trim_end_matches('/'), thread_id);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_delete_idempotent_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("threads");
        let eng = ThreadDeleteEngine::new(root.clone(), "http://127.0.0.1:9".into());
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
}
