//! `OutputParse -> CommandNormalize -> CommandValidate -> DispatchPlan` (LangChain/LangGraph routing layer).
//!
//! Structured-output strategy objects mirror LangChain v1 `ProviderStrategy` / `ToolStrategy` naming;
//! wiring into provider adapters is out of scope for the ports crate.

use crate::engine_command::EngineCommand;
use crate::interrupt::InterruptKind;
use crate::LlmTurnOutput;

/// Executable plan consumed by the runtime dispatch layer.
#[derive(Debug, Clone)]
pub struct DispatchPlan {
    pub command: EngineCommand,
}

/// Provider-native structured output policy (semantic placeholder for adapter binding).
#[derive(Debug, Clone, Default)]
pub struct ProviderStrategy {
    pub prefer_json_schema: bool,
}

/// Tool-calling structured output policy (semantic placeholder for adapter binding).
#[derive(Debug, Clone, Default)]
pub struct ToolStrategy {
    pub strict_validation: bool,
}

/// Options for [`build_dispatch_plan_with_options`] (provider/tool policy hooks).
#[derive(Debug, Clone, Default)]
pub struct BuildDispatchPlanOptions {
    pub provider: ProviderStrategy,
    pub tool: ToolStrategy,
}

/// Opaque parse step: surface is already [`LlmTurnOutput`] from the model port.
#[must_use]
pub fn parse_model_output(out: &LlmTurnOutput) -> &LlmTurnOutput {
    out
}

#[must_use]
pub fn classify_raw(out: &LlmTurnOutput) -> EngineCommand {
    if out.needs_clarification {
        return EngineCommand::Interrupt {
            kind: InterruptKind::Clarification { prompt: out.clarification_prompt.clone() },
        };
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
pub fn normalize_command(cmd: EngineCommand, out: &LlmTurnOutput) -> EngineCommand {
    if out.needs_clarification {
        return EngineCommand::Interrupt {
            kind: InterruptKind::Clarification { prompt: out.clarification_prompt.clone() },
        };
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

/// Build a validated dispatch plan from one model turn (default strategies).
pub fn build_dispatch_plan(out: &LlmTurnOutput) -> Result<DispatchPlan, &'static str> {
    build_dispatch_plan_with_options(out, &BuildDispatchPlanOptions::default())
}

/// Build a validated dispatch plan with structured-output strategy hooks.
pub fn build_dispatch_plan_with_options(
    out: &LlmTurnOutput,
    opts: &BuildDispatchPlanOptions,
) -> Result<DispatchPlan, &'static str> {
    let _parsed = parse_model_output(out);
    let cmd = classify_raw(_parsed);
    let cmd = normalize_command(cmd, _parsed);
    validate_engine_command_invariants(&cmd)?;
    apply_strategy_invariants(opts, _parsed, &cmd)?;
    let _prefer_schema = opts.provider.prefer_json_schema;
    Ok(DispatchPlan { command: cmd })
}

fn apply_strategy_invariants(
    opts: &BuildDispatchPlanOptions,
    _out: &LlmTurnOutput,
    cmd: &EngineCommand,
) -> Result<(), &'static str> {
    if !opts.tool.strict_validation {
        return Ok(());
    }
    if let EngineCommand::ToolCalls { calls, .. } = cmd {
        let mut seen = std::collections::HashSet::new();
        for c in calls {
            if !seen.insert(c.call_id.as_str()) {
                return Err("strict_validation: duplicate tool call_id in batch");
            }
        }
    }
    Ok(())
}

/// Invariants expected after normalization (tests and defensive checks).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SubtaskPlan, SubtaskSpec, ToolCallSpec};
    use serde_json::json;

    #[test]
    fn strict_validation_rejects_duplicate_call_ids() {
        let out = LlmTurnOutput {
            tool_calls: vec![
                ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "dup".into() },
                ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "dup".into() },
            ],
            ..Default::default()
        };
        let opts = BuildDispatchPlanOptions {
            tool: ToolStrategy { strict_validation: true },
            ..Default::default()
        };
        let err = build_dispatch_plan_with_options(&out, &opts).expect_err("dup ids");
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn pipeline_clarification_wins() {
        let out = LlmTurnOutput {
            needs_clarification: true,
            tool_calls: vec![ToolCallSpec {
                name: "x".into(),
                args: json!({}),
                call_id: "1".into(),
            }],
            ..Default::default()
        };
        let plan = build_dispatch_plan(&out).expect("valid");
        assert!(matches!(
            plan.command,
            EngineCommand::Interrupt { kind: InterruptKind::Clarification { .. } }
        ));
    }

    /// R2 行为矩阵：非 strict 时重复 call_id 仍可通过（由宿主/模型约束）。
    #[test]
    fn tool_strategy_non_strict_allows_duplicate_call_ids() {
        let out = LlmTurnOutput {
            tool_calls: vec![
                ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "dup".into() },
                ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "dup".into() },
            ],
            ..Default::default()
        };
        let opts = BuildDispatchPlanOptions {
            tool: ToolStrategy { strict_validation: false },
            ..Default::default()
        };
        let plan = build_dispatch_plan_with_options(&out, &opts).expect("valid");
        assert!(matches!(plan.command, EngineCommand::ToolCalls { .. }));
    }

    #[test]
    fn provider_strategy_round_trip_does_not_break_dispatch() {
        let out = LlmTurnOutput { assistant_text: Some("x".into()), ..Default::default() };
        let opts = BuildDispatchPlanOptions {
            provider: ProviderStrategy { prefer_json_schema: true },
            ..Default::default()
        };
        let plan = build_dispatch_plan_with_options(&out, &opts).expect("valid");
        assert!(matches!(plan.command, EngineCommand::TextAndMemory { .. }));
    }

    /// 互斥：子代理计划优先于同轮工具调用（与 classify_raw 优先级一致）。
    #[test]
    fn subagent_wins_over_tools_in_same_turn() {
        let out = LlmTurnOutput {
            subtask_plan: Some(SubtaskPlan {
                tasks: vec![SubtaskSpec { goal: "g".into(), input: json!({}), budget_steps: 1 }],
            }),
            tool_calls: vec![ToolCallSpec {
                name: "echo".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            ..Default::default()
        };
        let plan = build_dispatch_plan(&out).expect("valid");
        assert!(matches!(plan.command, EngineCommand::Subagent { .. }));
    }
}
