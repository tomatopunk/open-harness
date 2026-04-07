//! Authoritative thread-scoped state for the agent loop.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use crate::ids::{RunId, StepSeq, ThreadId};
use crate::interrupt::{InterruptSnapshot, ResumeCursor};
use crate::schema::THREAD_STATE_SCHEMA_VERSION;
use crate::task::TaskEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolInvocationRecord {
    pub tool_name: String,
    pub args: Value,
    pub invocation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolResultRecord {
    pub invocation_id: String,
    pub tool_name: String,
    pub ok: bool,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanState {
    pub active: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClarificationState {
    pub pending: bool,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryWorkingSet {
    pub snippets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryCommit {
    pub facts: Vec<String>,
    pub committed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentTaskRecord {
    pub task_id: uuid::Uuid,
    pub goal: String,
    pub status: String,
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactRef {
    pub path: String,
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GovernanceMarks {
    pub policy_version: Option<String>,
    pub tags: Vec<String>,
}

/// One durable write applied to a logical channel during `apply_writes` (BSP).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingWriteRecord {
    pub channel: String,
    pub version: u64,
    pub node_id: String,
    /// LangGraph-style task identity: the [`crate::task::TaskEnvelope::id`] for this write when known.
    #[serde(default)]
    pub task_id: Option<Uuid>,
}

/// Pregel-style channel versions for scheduling / resume (LangGraph-inspired).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PregelMeta {
    /// Monotonic version per logical channel name (e.g. `"state"`).
    #[serde(default)]
    pub channel_versions: HashMap<String, u64>,
    /// `node_id` → channel → last seen version after that node ran.
    #[serde(default)]
    pub versions_seen: HashMap<String, HashMap<String, u64>>,
    /// Superstep counter within the current run (incremented on durable commits).
    #[serde(default)]
    pub superstep_seq: u64,
    /// Tasks prepared for the current superstep (PULL/PUSH); cleared after `apply_writes` phases.
    #[serde(default)]
    pub staged_tasks: Vec<TaskEnvelope>,
    /// Writes staged in the current execute phase; consumed when bumping channel after a node.
    #[serde(default)]
    pub pending_write_queue: Vec<PendingWriteRecord>,
    /// Last interrupt snapshot (cleared on resume after user input).
    #[serde(default)]
    pub interrupt: Option<InterruptSnapshot>,
    /// Last successful resume cursor (audit / replay; set when clearing [`Self::interrupt`]).
    #[serde(default)]
    pub last_resume_at: Option<ResumeCursor>,
}

impl PregelMeta {
    /// Bump `channel` and return the new version.
    pub fn bump_channel(&mut self, channel: &str) -> u64 {
        let v = self.channel_versions.entry(channel.to_string()).or_insert(0);
        *v = v.saturating_add(1);
        *v
    }

    /// Record that `node_id` observed `channel` at `version`.
    pub fn mark_node_seen(&mut self, node_id: &str, channel: &str, version: u64) {
        self.versions_seen
            .entry(node_id.to_string())
            .or_default()
            .insert(channel.to_string(), version);
    }
}

/// Full thread state snapshot (checkpoint payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadState {
    /// Frozen contract version for this snapshot.
    #[serde(default = "default_thread_state_schema")]
    pub state_schema_version: u32,
    pub thread_id: ThreadId,
    pub active_run_id: Option<RunId>,
    pub messages: Vec<ChatMessage>,
    pub tool_invocations: Vec<ToolInvocationRecord>,
    pub tool_results: Vec<ToolResultRecord>,
    pub todos: Vec<TodoItem>,
    pub plan_state: PlanState,
    pub clarification_state: ClarificationState,
    pub memory_working_set: MemoryWorkingSet,
    pub memory_commits: Vec<MemoryCommit>,
    pub subagent_tasks: Vec<SubagentTaskRecord>,
    pub artifacts: Vec<ArtifactRef>,
    pub governance_marks: GovernanceMarks,
    pub step_seq: StepSeq,
    /// Pregel scheduling metadata (V2 engine).
    #[serde(default)]
    pub pregel: PregelMeta,
}

fn default_thread_state_schema() -> u32 {
    THREAD_STATE_SCHEMA_VERSION
}

impl Default for ThreadState {
    fn default() -> Self {
        Self {
            state_schema_version: THREAD_STATE_SCHEMA_VERSION,
            thread_id: ThreadId::default(),
            active_run_id: None,
            messages: Vec::new(),
            tool_invocations: Vec::new(),
            tool_results: Vec::new(),
            todos: Vec::new(),
            plan_state: PlanState::default(),
            clarification_state: ClarificationState::default(),
            memory_working_set: MemoryWorkingSet::default(),
            memory_commits: Vec::new(),
            subagent_tasks: Vec::new(),
            artifacts: Vec::new(),
            governance_marks: GovernanceMarks::default(),
            step_seq: StepSeq::default(),
            pregel: PregelMeta::default(),
        }
    }
}

impl ThreadState {
    #[must_use]
    pub fn new(thread_id: ThreadId) -> Self {
        Self {
            state_schema_version: THREAD_STATE_SCHEMA_VERSION,
            thread_id,
            active_run_id: None,
            messages: Vec::new(),
            tool_invocations: Vec::new(),
            tool_results: Vec::new(),
            todos: Vec::new(),
            plan_state: PlanState::default(),
            clarification_state: ClarificationState::default(),
            memory_working_set: MemoryWorkingSet::default(),
            memory_commits: Vec::new(),
            subagent_tasks: Vec::new(),
            artifacts: Vec::new(),
            governance_marks: GovernanceMarks::default(),
            step_seq: StepSeq::initial(),
            pregel: PregelMeta::default(),
        }
    }
}
