//! Async ports: LLM, tools, memory, skills, subagents, checkpoints, thread state.

use async_trait::async_trait;
use serde_json::Value;

use crate::checkpoint::CheckpointRecord;
use crate::error::{PortError, PortResult};
use crate::ids::{RunId, ThreadId};
use crate::thread_state::ThreadState;
use crate::tool_manifest::{ToolAssemblyPolicy, ToolManifest};

/// LLM turn context (opaque policy blob + messages).
#[derive(Debug, Clone, Default)]
pub struct LlmTurnContext {
    pub run_id: RunId,
    pub thread_id: ThreadId,
    pub messages: Vec<Value>,
    pub system_prompt: Option<String>,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolCallSpec {
    pub name: String,
    pub args: Value,
    pub call_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct SubtaskSpec {
    pub goal: String,
    pub input: Value,
    pub budget_steps: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SubtaskPlan {
    pub tasks: Vec<SubtaskSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct LlmTurnOutput {
    /// Final assistant text when no tool/subtask path.
    pub assistant_text: Option<String>,
    pub tool_calls: Vec<ToolCallSpec>,
    pub subtask_plan: Option<SubtaskPlan>,
    pub needs_clarification: bool,
    pub clarification_prompt: Option<String>,
    pub finish_turn: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryContext {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub query: String,
    pub state: ThreadState,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryDelta {
    pub facts: Vec<String>,
    pub snippets: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillContext {
    pub thread_id: ThreadId,
    pub enabled_skill_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillInjection {
    pub preamble: String,
    pub resolved_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentMergeContext {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub state: ThreadState,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentResult {
    pub task_id: uuid::Uuid,
    pub ok: bool,
    pub output: Value,
}

#[async_trait]
pub trait LLMPort: Send + Sync {
    async fn infer_turn(&self, ctx: LlmTurnContext) -> PortResult<LlmTurnOutput>;
}

#[async_trait]
pub trait ToolPort: Send + Sync {
    fn manifests(&self) -> Vec<ToolManifest>;

    async fn invoke(
        &self,
        run_id: RunId,
        thread_id: ThreadId,
        call: &ToolCallSpec,
    ) -> PortResult<Value>;

    /// Dynamic assembly for this turn.
    fn assemble(&self, policy: &ToolAssemblyPolicy) -> Vec<ToolManifest> {
        let all = self.manifests();
        let resolved = policy.resolve(&all);
        resolved.into_iter().cloned().collect()
    }
}

#[async_trait]
pub trait MemoryPort: Send + Sync {
    async fn retrieve(&self, ctx: &MemoryContext) -> PortResult<Vec<String>>;
    async fn extract_and_commit(&self, state: &mut ThreadState) -> PortResult<MemoryDelta>;
}

#[async_trait]
pub trait SkillPort: Send + Sync {
    async fn inject(&self, ctx: &SkillContext) -> PortResult<SkillInjection>;
}

#[async_trait]
pub trait SubagentPort: Send + Sync {
    async fn execute_plan(
        &self,
        run_id: RunId,
        thread_id: ThreadId,
        plan: &SubtaskPlan,
        state: &ThreadState,
    ) -> PortResult<Vec<SubagentResult>>;

    async fn merge(
        &self,
        ctx: &SubagentMergeContext,
        results: &[SubagentResult],
    ) -> PortResult<ThreadState>;
}

#[async_trait]
pub trait CheckpointPort: Send + Sync {
    async fn save(&self, record: CheckpointRecord) -> PortResult<()>;
    async fn load_latest(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
    ) -> PortResult<Option<CheckpointRecord>>;
}

#[async_trait]
pub trait ThreadStatePort: Send + Sync {
    async fn load(&self, thread_id: ThreadId) -> PortResult<ThreadState>;
    async fn save(&self, state: &ThreadState) -> PortResult<()>;
}

/// Helper: validate unknown tool names against assembled manifests.
#[must_use]
pub fn tool_allowed(name: &str, manifests: &[ToolManifest]) -> bool {
    manifests.iter().any(|m| m.name == name)
}

impl From<PortError> for String {
    fn from(e: PortError) -> Self {
        e.to_string()
    }
}
