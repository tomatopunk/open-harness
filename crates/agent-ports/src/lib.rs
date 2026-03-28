//! Agent execution contracts: thread state, checkpoints, events, tool manifests, and ports.
//!
//! This crate is intentionally free of LangChain/rig/provider SDKs. Adapters live in
//! `agent-adapters`.

pub mod checkpoint;
pub mod checkpoint_engine;
pub mod command_pipeline;
pub mod engine_command;
pub mod error;
pub mod events;
pub mod ids;
pub mod interrupt;
pub mod llm_routing;
pub mod ports;
pub mod schema;
pub mod state_effect;
pub mod task;
pub mod thread_state;
pub mod tool_manifest;

pub use checkpoint::CheckpointRecord;
pub use checkpoint_engine::EngineCheckpointExtensions;
pub use command_pipeline::{
    build_dispatch_plan, build_dispatch_plan_with_options, validate_engine_command_invariants,
    BuildDispatchPlanOptions, DispatchPlan, ProviderStrategy, ToolStrategy,
};
pub use engine_command::EngineCommand;
pub use error::{PortError, PortResult};
pub use events::{AgentEvent, EventSink, LoopStage, StepKind};
pub use ids::{CheckpointId, RunId, StepSeq, ThreadId};
pub use interrupt::{InterruptKind, InterruptSnapshot, ResumeCursor};
pub use llm_routing::classify_llm_routing;
pub use ports::{
    tool_allowed, CheckpointPort, LLMPort, LlmTurnContext, LlmTurnOutput, MemoryContext,
    MemoryDelta, MemoryPort, SkillContext, SkillInjection, SkillPort, SubagentExecuteParams,
    SubagentMergeContext, SubagentPort, SubagentResult, SubtaskPlan, SubtaskSpec, ThreadStatePort,
    ToolCallSpec, ToolPort,
};
pub use schema::{
    AGENT_EVENT_SCHEMA_VERSION, CHECKPOINT_RECORD_SCHEMA_VERSION, THREAD_STATE_SCHEMA_VERSION,
};
pub use state_effect::{apply_state_effects, tool_round_from_calls, StateEffect};
pub use task::{TaskEnvelope, TaskKind};
pub use thread_state::{
    ArtifactRef, ChatMessage, ClarificationState, GovernanceMarks, MemoryCommit, MemoryWorkingSet,
    PendingWriteRecord, PlanState, PregelMeta, SubagentTaskRecord, ThreadState, TodoItem,
    ToolInvocationRecord, ToolResultRecord,
};
pub use tool_manifest::{RiskLevel, SideEffectClass, ToolAssemblyPolicy, ToolManifest};
