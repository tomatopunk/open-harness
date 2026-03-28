//! Explicit classification of [`crate::LlmTurnOutput`] into a single [`crate::EngineCommand`].
//!
//! Replaces implicit priority tables with a named entry point and validation hooks.

use crate::{EngineCommand, LlmTurnOutput};

/// Classify model output into exactly one engine command (same priority as legacy `EngineCommand::from_llm_output`).
///
/// Clarification always wins; empty plans / empty tool lists are normalized to the text path.
#[must_use]
pub fn classify_llm_routing(out: &LlmTurnOutput) -> EngineCommand {
    let cmd = classify_llm_routing_raw(out);
    let cmd = normalize_engine_command(cmd, out);
    debug_assert!(validate_engine_command_invariants(&cmd).is_ok());
    cmd
}

/// Invariants expected after [`classify_llm_routing`] (tests and defensive checks).
pub fn validate_engine_command_invariants(cmd: &EngineCommand) -> Result<(), &'static str> {
    match cmd {
        EngineCommand::Subagent { plan, .. } if plan.tasks.is_empty() => {
            Err("subagent command must not have empty task list after normalization")
        }
        EngineCommand::ToolCalls { calls, .. } if calls.is_empty() => {
            Err("tool command must not have empty calls after normalization")
        }
        _ => Ok(()),
    }
}

#[must_use]
fn classify_llm_routing_raw(out: &LlmTurnOutput) -> EngineCommand {
    if out.needs_clarification {
        return EngineCommand::ClarifyExit;
    }
    if let Some(plan) = &out.subtask_plan {
        return EngineCommand::Subagent { plan: plan.clone(), finish_turn: out.finish_turn };
    }
    if !out.tool_calls.is_empty() {
        return EngineCommand::ToolCalls {
            calls: out.tool_calls.clone(),
            finish_turn: out.finish_turn,
        };
    }
    EngineCommand::TextAndMemory {
        assistant_text: out.assistant_text.clone(),
        finish_turn: out.finish_turn,
    }
}

#[must_use]
fn normalize_engine_command(cmd: EngineCommand, out: &LlmTurnOutput) -> EngineCommand {
    if out.needs_clarification {
        return EngineCommand::ClarifyExit;
    }
    match cmd {
        EngineCommand::Subagent { ref plan, finish_turn } if plan.tasks.is_empty() => {
            EngineCommand::TextAndMemory { assistant_text: out.assistant_text.clone(), finish_turn }
        }
        EngineCommand::ToolCalls { ref calls, finish_turn } if calls.is_empty() => {
            EngineCommand::TextAndMemory { assistant_text: out.assistant_text.clone(), finish_turn }
        }
        other => other,
    }
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
        let cmd = classify_llm_routing(&out);
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
        assert!(matches!(classify_llm_routing(&out), EngineCommand::ClarifyExit));
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
        assert!(matches!(classify_llm_routing(&out), EngineCommand::Subagent { .. }));
    }
}
