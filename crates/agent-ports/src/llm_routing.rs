//! Explicit classification of [`crate::LlmTurnOutput`] into a single [`crate::EngineCommand`].
//!
//! Delegates to [`crate::command_pipeline`]; kept for stable import paths.

pub use crate::command_pipeline::validate_engine_command_invariants;

use crate::{EngineCommand, LlmTurnOutput};

/// Classify model output into exactly one engine command (same priority as [`crate::command_pipeline::build_dispatch_plan`]).
///
/// Clarification always wins; empty plans / empty tool lists are normalized to the text path.
pub fn classify_llm_routing(out: &LlmTurnOutput) -> Result<EngineCommand, &'static str> {
    EngineCommand::try_from_llm_output(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SubtaskPlan, SubtaskSpec, ToolCallSpec};
    use serde_json::json;

    #[test]
    fn empty_subagent_plan_normalizes_to_text() {
        let out = LlmTurnOutput {
            subtask_plan: Some(SubtaskPlan { tasks: vec![] }),
            finish_turn: false,
            ..Default::default()
        };
        let cmd = classify_llm_routing(&out).expect("valid");
        assert!(matches!(
            cmd,
            EngineCommand::TextAndMemory { assistant_text: None, finish_turn: false }
        ));
    }

    #[test]
    fn clarification_wins_over_tools() {
        let out = LlmTurnOutput {
            needs_clarification: true,
            tool_calls: vec![ToolCallSpec {
                name: "x".into(),
                args: json!({}),
                call_id: "1".into(),
            }],
            ..Default::default()
        };
        assert!(matches!(
            classify_llm_routing(&out).expect("valid"),
            EngineCommand::Interrupt {
                kind: crate::interrupt::InterruptKind::Clarification { .. }
            }
        ));
    }

    #[test]
    fn subtask_plan_beats_tool_calls() {
        let out = LlmTurnOutput {
            subtask_plan: Some(SubtaskPlan {
                tasks: vec![SubtaskSpec { goal: "g".into(), input: json!({}), budget_steps: 1 }],
            }),
            tool_calls: vec![ToolCallSpec {
                name: "x".into(),
                args: json!({}),
                call_id: "1".into(),
            }],
            ..Default::default()
        };
        assert!(matches!(
            classify_llm_routing(&out).expect("valid"),
            EngineCommand::Subagent { .. }
        ));
    }
}
