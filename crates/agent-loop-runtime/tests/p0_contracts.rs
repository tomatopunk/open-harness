//! P0 契约：超步任务信封顺序、路由单入口、与 `EngineCommand` 对齐。

use agent_loop_runtime::superstep_kernel;
use agent_ports::{classify_llm_routing, EngineCommand, LlmTurnOutput, SubtaskPlan, SubtaskSpec};
use agent_ports::{TaskKind, ThreadId, ThreadState, ToolCallSpec};
use serde_json::json;

#[test]
fn golden_staged_pull_sequence_matches_engine_phase_nodes() {
    let mut st = ThreadState::new(ThreadId::new_v4());
    superstep_kernel::prepare_tasks(&mut st);
    superstep_kernel::prepare::prepare_pull_task(&mut st, "lead");
    superstep_kernel::prepare::prepare_pull_task(&mut st, "premodel");
    superstep_kernel::prepare::prepare_pull_task(&mut st, "model");
    superstep_kernel::prepare::prepare_pull_task(&mut st, "postmodel");
    assert_eq!(st.pregel.staged_tasks.len(), 4);
    for (i, expected) in ["lead", "premodel", "model", "postmodel"].iter().enumerate() {
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
    for (i, slot) in [0u32, 1].iter().enumerate() {
        match &st.pregel.staged_tasks[i].kind {
            TaskKind::Push { fanout_id, slot: s } => {
                assert_eq!(fanout_id, "tool_invoke");
                assert_eq!(*s, *slot);
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
        let a = EngineCommand::from_llm_output(&out);
        let b = classify_llm_routing(&out);
        assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "from_llm_output vs classify_llm_routing mismatch"
        );
    }
}
