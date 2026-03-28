//! Authoritative thread-scoped state for the agent loop.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{RunId, StepSeq, ThreadId};

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

/// Full thread state snapshot (checkpoint payload).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadState {
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
}

impl ThreadState {
    #[must_use]
    pub fn new(thread_id: ThreadId) -> Self {
        Self {
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
        }
    }
}
