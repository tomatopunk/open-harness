//! First-class routing command after one model turn (LangGraph `Command`-style routing surface).
//!
//! Maps [`crate::LlmTurnOutput`] to a single branch; priority matches the inner loop contract:
//! clarification > subagent plan > tool calls > text + memory.

use crate::{LlmTurnOutput, SubtaskPlan, ToolCallSpec};

/// Executable routing decision for the inner agent engine (post-model).
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// Exit run and wait for user clarification (HITL-style pause).
    ClarifyExit,
    /// Run subagent plan, then optional `finish_turn`.
    Subagent { plan: SubtaskPlan, finish_turn: bool },
    /// Run tool calls (possibly parallelized by runtime), then optional `finish_turn`.
    ToolCalls { calls: Vec<ToolCallSpec>, finish_turn: bool },
    /// Append assistant text, memory commit, then optional `finish_turn`.
    TextAndMemory { assistant_text: Option<String>, finish_turn: bool },
}

impl EngineCommand {
    /// Classify model output into exactly one command (explicit router; see [`crate::llm_routing`]).
    #[must_use]
    pub fn from_llm_output(out: &LlmTurnOutput) -> Self {
        crate::llm_routing::classify_llm_routing(out)
    }
}
