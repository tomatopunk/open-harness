//! R0 四路径 golden：与运行时一致，经 `build_dispatch_plan_with_options`（Command IR 主路径）。

use agent_ports::{
    build_dispatch_plan_with_options, validate_engine_command_invariants, BuildDispatchPlanOptions,
    EngineCommand, InterruptKind, LlmTurnOutput, SubtaskPlan, SubtaskSpec, ToolCallSpec,
};
use serde_json::json;

fn opts() -> BuildDispatchPlanOptions {
    BuildDispatchPlanOptions::default()
}

#[test]
fn golden_clarify_via_command_ir() {
    let out = LlmTurnOutput {
        needs_clarification: true,
        clarification_prompt: Some("?".into()),
        ..Default::default()
    };
    let plan = build_dispatch_plan_with_options(&out, &opts()).expect("valid");
    assert!(matches!(
        plan.command,
        EngineCommand::Interrupt { kind: InterruptKind::Clarification { .. } }
    ));
    validate_engine_command_invariants(&plan.command).expect("inv");
}

#[test]
fn golden_subagent_via_command_ir() {
    let out = LlmTurnOutput {
        subtask_plan: Some(SubtaskPlan {
            tasks: vec![SubtaskSpec { goal: "g".into(), input: json!({}), budget_steps: 1 }],
        }),
        ..Default::default()
    };
    let plan = build_dispatch_plan_with_options(&out, &opts()).expect("valid");
    assert!(matches!(plan.command, EngineCommand::Subagent { .. }));
    validate_engine_command_invariants(&plan.command).expect("inv");
}

#[test]
fn golden_tools_via_command_ir() {
    let out = LlmTurnOutput {
        tool_calls: vec![ToolCallSpec {
            name: "echo".into(),
            args: json!({}),
            call_id: "c1".into(),
        }],
        finish_turn: true,
        ..Default::default()
    };
    let plan = build_dispatch_plan_with_options(&out, &opts()).expect("valid");
    assert!(matches!(plan.command, EngineCommand::ToolCalls { .. }));
    validate_engine_command_invariants(&plan.command).expect("inv");
}

#[test]
fn golden_text_via_command_ir() {
    let out = LlmTurnOutput {
        assistant_text: Some("hello".into()),
        finish_turn: false,
        ..Default::default()
    };
    let plan = build_dispatch_plan_with_options(&out, &opts()).expect("valid");
    assert!(matches!(plan.command, EngineCommand::TextAndMemory { .. }));
    validate_engine_command_invariants(&plan.command).expect("inv");
}
