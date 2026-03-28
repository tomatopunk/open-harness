//! Thread / Run / Step lifecycle and resume.

use agent_ports::{RunId, StepSeq, ThreadId, ThreadState};
use uuid::Uuid;

use crate::checkpoint_store::{make_checkpoint, MemoryCheckpointStore};
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

/// Minimal graph runtime: tracks threads, runs, and checkpoints in memory.
#[derive(Debug, Clone)]
pub struct GraphRuntime {
    checkpoints: MemoryCheckpointStore,
}

impl Default for GraphRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self { checkpoints: MemoryCheckpointStore::new() }
    }

    /// Start a new run on a thread (forks state from latest checkpoint if any).
    pub fn start_run(&self, thread_id: ThreadId, mut base: ThreadState) -> GraphResult<RunHandle> {
        if base.thread_id != thread_id {
            return Err(GraphRuntimeError::InvalidTransition("thread_id mismatch".into()));
        }
        let run_id = RunId::new_v4();
        base.active_run_id = Some(run_id);
        base.step_seq = StepSeq::initial();
        let cp = make_checkpoint(thread_id, run_id, base.clone());
        self.checkpoints.push(cp);
        Ok(RunHandle { thread_id, run_id, status: RunStatus::Running, cursor: StepSeq::initial() })
    }

    /// Resume using latest checkpoint for this run.
    pub fn resume_run(&self, thread_id: ThreadId, run_id: RunId) -> GraphResult<ThreadState> {
        let Some(cp) = self.checkpoints.latest(thread_id, run_id) else {
            return Err(GraphRuntimeError::RunNotFound(format!("{run_id:?}")));
        };
        Ok(cp.state)
    }

    /// Advance step cursor and persist checkpoint after external state mutation.
    pub fn commit_step(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        mut state: ThreadState,
    ) -> GraphResult<StepSeq> {
        state.thread_id = thread_id;
        state.active_run_id = Some(run_id);
        state.step_seq = state.step_seq.next();
        let cp = make_checkpoint(thread_id, run_id, state);
        let seq = cp.step_seq;
        self.checkpoints.push(cp);
        Ok(seq)
    }

    /// Complete run (terminal checkpoint with same step_seq semantics).
    pub fn complete_run(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        mut state: ThreadState,
        reason: &str,
    ) -> GraphResult<RunHandle> {
        state.active_run_id = Some(run_id);
        let cp = make_checkpoint(thread_id, run_id, state);
        let cursor = cp.step_seq;
        self.checkpoints.push(cp);
        Ok(RunHandle {
            thread_id,
            run_id,
            status: RunStatus::Completed { reason: reason.to_string() },
            cursor,
        })
    }

    pub fn checkpoints(&self) -> &MemoryCheckpointStore {
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
    use agent_ports::ChatMessage;

    #[test]
    fn start_commit_resume() {
        let rt = GraphRuntime::new();
        let tid = ThreadId::new_v4();
        let base = ThreadState::new(tid);
        let run = rt.start_run(tid, base).expect("start");
        let mut st = rt.resume_run(tid, run.run_id).expect("resume");
        st.messages.push(ChatMessage { role: "user".into(), content: serde_json::json!("hi") });
        let seq = rt.commit_step(tid, run.run_id, st.clone()).expect("commit");
        assert_eq!(seq.0, 1);
        let st2 = rt.resume_run(tid, run.run_id).expect("resume2");
        assert_eq!(st2.step_seq.0, 1);
    }
}
