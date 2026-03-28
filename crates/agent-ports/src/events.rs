//! Unified streaming event protocol for runs.

use crate::ids::{CheckpointId, RunId, StepSeq, ThreadId};
use crate::schema::AGENT_EVENT_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};

/// Explicit stage within one turn of the inner agent loop (observability / recovery boundaries).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LoopStage {
    PreModel,
    Model,
    PostModel,
    ClarifyExit,
    SubagentExec,
    ToolExec,
    MemoryCommit,
    StateCommit,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    RunStarted {
        thread_id: ThreadId,
        run_id: RunId,
    },
    RunResumed {
        thread_id: ThreadId,
        run_id: RunId,
        from_step: StepSeq,
    },
    RunCompleted {
        run_id: RunId,
        reason: String,
    },
    RunFailed {
        run_id: RunId,
        message: String,
    },
    StepStarted {
        run_id: RunId,
        step_seq: StepSeq,
        kind: StepKind,
    },
    StepFinished {
        run_id: RunId,
        step_seq: StepSeq,
        kind: StepKind,
    },
    /// Fine-grained stage within the current step (inner engine).
    StageStarted {
        run_id: RunId,
        step_seq: StepSeq,
        stage: LoopStage,
    },
    StageFinished {
        run_id: RunId,
        step_seq: StepSeq,
        stage: LoopStage,
    },
    LlmDelta {
        run_id: RunId,
        text: String,
    },
    LlmCompleted {
        run_id: RunId,
    },
    ToolSelected {
        run_id: RunId,
        tool_name: String,
    },
    ToolExecuted {
        run_id: RunId,
        tool_name: String,
        ok: bool,
    },
    SubagentPlanned {
        run_id: RunId,
        task_count: usize,
    },
    SubagentTaskStarted {
        run_id: RunId,
        task_id: uuid::Uuid,
        goal: String,
    },
    SubagentTaskCompleted {
        run_id: RunId,
        task_id: uuid::Uuid,
        ok: bool,
    },
    SubagentTaskTimedOut {
        run_id: RunId,
        task_id: uuid::Uuid,
    },
    SubagentCompleted {
        run_id: RunId,
        task_id: uuid::Uuid,
    },
    StateCommitted {
        run_id: RunId,
        step_seq: StepSeq,
    },
    CheckpointSaved {
        run_id: RunId,
        checkpoint_id: CheckpointId,
        step_seq: StepSeq,
    },
    MemoryUpdated,
    SkillInjected {
        skill_names: Vec<String>,
    },
    ClarificationRequested {
        run_id: RunId,
        prompt: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Llm,
    Tool,
    Subagent,
    StateCommit,
    Merge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSink {
    #[serde(default = "default_event_schema")]
    pub event_schema_version: u32,
    pub events: Vec<AgentEvent>,
}

fn default_event_schema() -> u32 {
    AGENT_EVENT_SCHEMA_VERSION
}

impl Default for EventSink {
    fn default() -> Self {
        Self { event_schema_version: AGENT_EVENT_SCHEMA_VERSION, events: Vec::new() }
    }
}

impl EventSink {
    pub fn push(&mut self, ev: AgentEvent) {
        self.events.push(ev);
    }
}
