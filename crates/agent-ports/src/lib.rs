//! Agent execution contracts: thread state, checkpoints, events, tool manifests, and ports.
//!
//! This crate is intentionally free of LangChain/rig/provider SDKs. Adapters live in
//! `agent-adapters`.

pub mod checkpoint;
pub mod error;
pub mod events;
pub mod ids;
pub mod ports;
pub mod thread_state;
pub mod tool_manifest;

pub use checkpoint::CheckpointRecord;
pub use error::{PortError, PortResult};
pub use events::{AgentEvent, EventSink, StepKind};
pub use ids::{CheckpointId, RunId, StepSeq, ThreadId};
pub use ports::{
    tool_allowed, CheckpointPort, LLMPort, LlmTurnContext, LlmTurnOutput, MemoryContext,
    MemoryDelta, MemoryPort, SkillContext, SkillInjection, SkillPort, SubagentMergeContext,
    SubagentPort, SubagentResult, SubtaskPlan, SubtaskSpec, ThreadStatePort, ToolCallSpec,
    ToolPort,
};
pub use thread_state::{
    ArtifactRef, ChatMessage, ClarificationState, GovernanceMarks, MemoryCommit, MemoryWorkingSet,
    PlanState, SubagentTaskRecord, ThreadState, TodoItem, ToolInvocationRecord, ToolResultRecord,
};
pub use tool_manifest::{RiskLevel, SideEffectClass, ToolAssemblyPolicy, ToolManifest};
