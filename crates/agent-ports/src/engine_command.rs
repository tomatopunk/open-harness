//! First-class routing command after one model turn (LangGraph `Command`-style routing surface).
//!
//! Maps [`crate::LlmTurnOutput`] to a single branch; priority matches the inner loop contract:
//! clarification > subagent plan > tool calls > text + memory.

use crate::interrupt::InterruptKind;
use crate::{LlmTurnOutput, SubtaskPlan, ToolCallSpec};

/// Executable routing decision for the inner agent engine (post-model).
#[derive(Debug, Clone)]
pub enum EngineCommand {
    /// Pause run until host resumes (clarification, approvals, …).
    Interrupt { kind: InterruptKind },
    /// Run subagent plan, then optional `finish_turn`.
    Subagent { plan: SubtaskPlan, finish_turn: bool },
    /// Run tool calls (possibly parallelized by runtime), then optional `finish_turn`.
    ToolCalls { calls: Vec<ToolCallSpec>, finish_turn: bool },
    /// Append assistant text, memory commit, then optional `finish_turn`.
    TextAndMemory { assistant_text: Option<String>, finish_turn: bool },
}

impl EngineCommand {
    /// Build a routing command; returns `Err` if invariants fail (should be rare after normalization).
    pub fn try_from_llm_output(out: &LlmTurnOutput) -> Result<Self, &'static str> {
        crate::command_pipeline::build_dispatch_plan(out).map(|p| p.command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCallSpec;
    use serde_json::json;

    #[test]
    fn try_from_matches_build_dispatch_plan() {
        let out = LlmTurnOutput {
            tool_calls: vec![ToolCallSpec {
                name: "echo".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            ..Default::default()
        };
        let a = EngineCommand::try_from_llm_output(&out).expect("valid");
        let b = crate::command_pipeline::build_dispatch_plan(&out).expect("valid").command;
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
}
