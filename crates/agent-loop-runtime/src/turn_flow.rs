//! Classification of LLM output into an [`EngineCommand`] (LangGraph-style routing).

pub use agent_ports::EngineCommand;

/// Back-compat alias for [`EngineCommand`].
pub type TurnOutcome = EngineCommand;

/// Maps model output to a single branch (priority: clarification > subagent > tools > text).
#[must_use]
pub fn classify_turn_outcome(out: &agent_ports::LlmTurnOutput) -> EngineCommand {
    EngineCommand::from_llm_output(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::{LlmTurnOutput, SubtaskPlan, SubtaskSpec, ToolCallSpec};
    use serde_json::json;

    #[test]
    fn classify_prefers_clarification() {
        let out = LlmTurnOutput {
            needs_clarification: true,
            subtask_plan: Some(SubtaskPlan { tasks: vec![SubtaskSpec::default()] }),
            ..Default::default()
        };
        assert!(matches!(classify_turn_outcome(&out), EngineCommand::ClarifyExit));
    }

    #[test]
    fn classify_prefers_subagent_over_tools() {
        let out = LlmTurnOutput {
            subtask_plan: Some(SubtaskPlan { tasks: vec![SubtaskSpec::default()] }),
            tool_calls: vec![ToolCallSpec {
                name: "x".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            ..Default::default()
        };
        assert!(matches!(classify_turn_outcome(&out), EngineCommand::Subagent { .. }));
    }

    #[test]
    fn classify_tools_when_no_subagent() {
        let out = LlmTurnOutput {
            tool_calls: vec![ToolCallSpec {
                name: "echo".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            finish_turn: true,
            ..Default::default()
        };
        match classify_turn_outcome(&out) {
            EngineCommand::ToolCalls { calls, finish_turn } => {
                assert_eq!(calls.len(), 1);
                assert!(finish_turn);
            }
            _ => panic!("expected tool path"),
        }
    }
}
