//! P0 契约：超步任务信封顺序、路由单入口、与 `EngineCommand` 对齐。

use agent_loop_runtime::runtime_spec::{DispatchPhaseNodes, LeadRuntimeSpec};
use agent_loop_runtime::superstep_kernel;
use agent_loop_runtime::{truncate_subtask_plan, RunBudget};
use agent_ports::{
    classify_llm_routing, validate_engine_command_invariants, EngineCommand, InterruptKind,
    LlmTurnOutput, SubtaskPlan, SubtaskSpec,
};
use agent_ports::{TaskKind, ThreadId, ThreadState, ToolCallSpec};
use serde_json::json;

#[test]
fn golden_staged_pull_sequence_matches_engine_phase_nodes() {
    let mut st = ThreadState::new(ThreadId::new_v4());
    superstep_kernel::prepare_tasks(&mut st);
    for node in LeadRuntimeSpec::main_phase_pull_order() {
        superstep_kernel::prepare::prepare_pull_task(&mut st, node);
    }
    assert_eq!(st.pregel.staged_tasks.len(), 4);
    for (i, expected) in LeadRuntimeSpec::main_phase_pull_order().iter().enumerate() {
        match &st.pregel.staged_tasks[i].kind {
            TaskKind::Pull { node_id } => assert_eq!(node_id, expected),
            other => panic!("expected Pull at {i}, got {other:?}"),
        }
    }
}

#[test]
fn golden_tool_fanout_stages_push_slots() {
    let mut st = ThreadState::new(ThreadId::new_v4());
    let calls = vec![
        ToolCallSpec { name: "a".into(), args: json!({}), call_id: "1".into() },
        ToolCallSpec { name: "b".into(), args: json!({}), call_id: "2".into() },
    ];
    superstep_kernel::prepare::prepare_tool_fanout(&mut st, &calls);
    assert_eq!(st.pregel.staged_tasks.len(), 2);
    let expected_ids = ["1", "2"];
    for (i, expected_id) in expected_ids.iter().enumerate() {
        match &st.pregel.staged_tasks[i].kind {
            TaskKind::Push { fanout_id, call_id } => {
                assert_eq!(fanout_id, "tool_invoke");
                assert_eq!(call_id, expected_id);
            }
            other => panic!("expected Push at {i}, got {other:?}"),
        }
    }
}

#[test]
fn golden_subagent_fanout_stages_push_slots() {
    let mut st = ThreadState::new(ThreadId::new_v4());
    superstep_kernel::prepare::prepare_subagent_fanout(&mut st, 3);
    assert_eq!(st.pregel.staged_tasks.len(), 3);
    for i in 0..3 {
        match &st.pregel.staged_tasks[i].kind {
            TaskKind::Push { fanout_id, call_id } => {
                assert_eq!(fanout_id, "subagent_task");
                assert_eq!(call_id, &format!("subagent:{i}"));
            }
            other => panic!("expected Push at {i}, got {other:?}"),
        }
    }
}

#[test]
fn routing_engine_command_matches_classify_llm_routing() {
    let cases = vec![
        LlmTurnOutput {
            needs_clarification: true,
            tool_calls: vec![ToolCallSpec {
                name: "x".into(),
                args: json!({}),
                call_id: "1".into(),
            }],
            ..Default::default()
        },
        LlmTurnOutput {
            subtask_plan: Some(SubtaskPlan {
                tasks: vec![SubtaskSpec { goal: "g".into(), input: json!({}), budget_steps: 1 }],
            }),
            tool_calls: vec![ToolCallSpec {
                name: "echo".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            ..Default::default()
        },
        LlmTurnOutput {
            tool_calls: vec![ToolCallSpec {
                name: "echo".into(),
                args: json!({}),
                call_id: "c".into(),
            }],
            finish_turn: true,
            ..Default::default()
        },
        LlmTurnOutput {
            assistant_text: Some("hi".into()),
            finish_turn: false,
            ..Default::default()
        },
    ];
    for out in cases {
        let a = EngineCommand::try_from_llm_output(&out).expect("valid");
        let b = classify_llm_routing(&out).expect("valid");
        assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "try_from_llm_output vs classify_llm_routing mismatch"
        );
        validate_engine_command_invariants(&a).expect("invariants");
    }
}

/// 四条路径：clarify / subagent / tools / text — 路由结果可枚举且满足不变量。
#[test]
fn four_routing_paths_are_distinct_and_valid() {
    let clarify =
        classify_llm_routing(&LlmTurnOutput { needs_clarification: true, ..Default::default() })
            .expect("valid");
    assert!(matches!(
        clarify,
        EngineCommand::Interrupt { kind: InterruptKind::Clarification { .. } }
    ));
    validate_engine_command_invariants(&clarify).expect("clarify invariants");

    let sub = classify_llm_routing(&LlmTurnOutput {
        subtask_plan: Some(SubtaskPlan {
            tasks: vec![SubtaskSpec { goal: "do".into(), input: json!({}), budget_steps: 2 }],
        }),
        ..Default::default()
    })
    .expect("valid");
    assert!(matches!(sub, EngineCommand::Subagent { .. }));
    validate_engine_command_invariants(&sub).expect("subagent invariants");

    let tools = classify_llm_routing(&LlmTurnOutput {
        tool_calls: vec![ToolCallSpec {
            name: "echo".into(),
            args: json!({}),
            call_id: "x".into(),
        }],
        ..Default::default()
    })
    .expect("valid");
    assert!(matches!(tools, EngineCommand::ToolCalls { .. }));
    validate_engine_command_invariants(&tools).expect("tools invariants");

    let text = classify_llm_routing(&LlmTurnOutput {
        assistant_text: Some("ok".into()),
        ..Default::default()
    })
    .expect("valid");
    assert!(matches!(text, EngineCommand::TextAndMemory { .. }));
    validate_engine_command_invariants(&text).expect("text invariants");
}

#[test]
fn subtask_truncation_respects_effective_cap() {
    let budget = RunBudget {
        max_subagent_tasks: 2,
        subagent_task_cap_per_response: 10,
        ..RunBudget::default()
    };
    let plan = SubtaskPlan {
        tasks: vec![
            SubtaskSpec { goal: "a".into(), input: json!({}), budget_steps: 1 },
            SubtaskSpec { goal: "b".into(), input: json!({}), budget_steps: 1 },
            SubtaskSpec { goal: "c".into(), input: json!({}), budget_steps: 1 },
        ],
    };
    let (trunc, was_trunc) = truncate_subtask_plan(plan, &budget);
    assert!(was_trunc);
    assert_eq!(trunc.tasks.len(), 2);
}

#[test]
fn lead_runtime_phase_order_contract_stable() {
    assert_eq!(
        LeadRuntimeSpec::main_phase_pull_order(),
        &["lead", "premodel", "model", "postmodel"]
    );
}

#[test]
fn dispatch_phase_node_ids_stable() {
    assert_eq!(DispatchPhaseNodes::CLARIFY, "dispatch_clarify");
    assert_eq!(DispatchPhaseNodes::SUBAGENT, "dispatch_subagent");
    assert_eq!(DispatchPhaseNodes::TOOLS, "dispatch_tools");
    assert_eq!(DispatchPhaseNodes::TEXT, "dispatch_text");
}
