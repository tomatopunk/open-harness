//! Thread / Run / Step lifecycle and resume.

use std::sync::Arc;

use agent_ports::{CheckpointPort, RunId, StepSeq, ThreadId, ThreadState};
use uuid::Uuid;

use crate::checkpoint_store::make_checkpoint;
use crate::error::{GraphResult, GraphRuntimeError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Pending,
    Running,
    Completed { reason: String },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct RunHandle {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub status: RunStatus,
    pub cursor: StepSeq,
}

/// Graph runtime: tracks threads, runs, and checkpoints via [`CheckpointPort`] (memory or durable).
#[derive(Clone)]
pub struct GraphRuntime {
    checkpoints: Arc<dyn CheckpointPort>,
}

impl GraphRuntime {
    #[must_use]
    pub fn new(checkpoints: Arc<dyn CheckpointPort>) -> Self {
        Self { checkpoints }
    }

    /// Start a new run on a thread (forks state from latest checkpoint if any).
    pub async fn start_run(
        &self,
        thread_id: ThreadId,
        mut base: ThreadState,
    ) -> GraphResult<RunHandle> {
        if base.thread_id != thread_id {
            return Err(GraphRuntimeError::InvalidTransition("thread_id mismatch".into()));
        }
        let run_id = RunId::new_v4();
        base.active_run_id = Some(run_id);
        base.step_seq = StepSeq::initial();
        let cp = make_checkpoint(thread_id, run_id, base, serde_json::json!({}));
        self.checkpoints
            .save(cp)
            .await
            .map_err(|e| GraphRuntimeError::Checkpoint(e.to_string()))?;
        Ok(RunHandle { thread_id, run_id, status: RunStatus::Running, cursor: StepSeq::initial() })
    }

    /// Resume using latest checkpoint for this run.
    pub async fn resume_run(&self, thread_id: ThreadId, run_id: RunId) -> GraphResult<ThreadState> {
        let Some(cp) = self
            .checkpoints
            .load_latest(thread_id, run_id)
            .await
            .map_err(|e| GraphRuntimeError::Checkpoint(e.to_string()))?
        else {
            return Err(GraphRuntimeError::RunNotFound(format!("{run_id:?}")));
        };
        Ok(cp.state)
    }

    /// Advance step cursor and persist checkpoint after external state mutation.
    /// Returns the new step sequence and the committed snapshot.
    pub async fn commit_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        mut state: ThreadState,
        metadata: serde_json::Value,
    ) -> GraphResult<(StepSeq, ThreadState)> {
        state.thread_id = thread_id;
        state.active_run_id = Some(run_id);
        state.step_seq = state.step_seq.next();
        let seq = state.step_seq;
        let cp = make_checkpoint(thread_id, run_id, state.clone(), metadata);
        self.checkpoints
            .save(cp)
            .await
            .map_err(|e| GraphRuntimeError::Checkpoint(e.to_string()))?;
        Ok((seq, state))
    }

    /// Complete run: advances step once and writes a terminal checkpoint (distinct seq from last commit).
    pub async fn complete_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        mut state: ThreadState,
        reason: &str,
    ) -> GraphResult<(RunHandle, ThreadState)> {
        state.thread_id = thread_id;
        state.active_run_id = Some(run_id);
        state.step_seq = state.step_seq.next();
        let cursor = state.step_seq;
        let cp = make_checkpoint(
            thread_id,
            run_id,
            state.clone(),
            serde_json::json!({ "terminal": true, "reason": reason }),
        );
        self.checkpoints
            .save(cp)
            .await
            .map_err(|e| GraphRuntimeError::Checkpoint(e.to_string()))?;
        let handle = RunHandle {
            thread_id,
            run_id,
            status: RunStatus::Completed { reason: reason.to_string() },
            cursor,
        };
        Ok((handle, state))
    }

    pub fn checkpoint_port(&self) -> &Arc<dyn CheckpointPort> {
        &self.checkpoints
    }
}

/// Parse thread id from string (for HTTP boundaries).
#[must_use]
pub fn parse_thread_id(s: &str) -> Option<ThreadId> {
    Uuid::parse_str(s).ok().map(ThreadId)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_adapters::MemoryCheckpointAdapter;
    use agent_ports::ChatMessage;

    #[tokio::test]
    async fn start_commit_resume() {
        let port: Arc<dyn CheckpointPort> = Arc::new(MemoryCheckpointAdapter::default());
        let rt = GraphRuntime::new(port);
        let tid = ThreadId::new_v4();
        let base = ThreadState::new(tid);
        let run = rt.start_run(tid, base).await.expect("start");
        let mut st = rt.resume_run(tid, run.run_id).await.expect("resume");
        st.messages.push(ChatMessage { role: "user".into(), content: serde_json::json!("hi") });
        let (seq, _) = rt
            .commit_step(tid, run.run_id, st.clone(), serde_json::json!({}))
            .await
            .expect("commit");
        assert_eq!(seq.0, 1);
        let st2 = rt.resume_run(tid, run.run_id).await.expect("resume2");
        assert_eq!(st2.step_seq.0, 1);
    }
}
