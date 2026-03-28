//! R1：task 粒度状态可序列化恢复、多轮 interrupt/resume、混合 staged 顺序与工具顺序确定性。

use agent_loop_runtime::superstep_kernel;
use agent_loop_runtime::{maybe_resume_from_interrupt, RunBudget};
use agent_ports::{
    InterruptKind, InterruptSnapshot, ResumeCursor, StepSeq, SubtaskPlan, SubtaskSpec, TaskKind,
    ThreadId, ThreadState, ToolCallSpec,
};
use serde_json::json;

#[test]
fn thread_state_serde_roundtrip_preserves_pregel_audit_fields() {
    let tid = ThreadId::new_v4();
    let mut st = ThreadState::new(tid);
    st.pregel.superstep_seq = 7;
    superstep_kernel::prepare_tasks(&mut st);
    let task_id = superstep_kernel::prepare::prepare_pull_task(&mut st, "lead");
    superstep_kernel::apply_writes_after_node(&mut st, "lead", Some(task_id));

    let json = serde_json::to_string(&st).expect("serialize");
    let back: ThreadState = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.pregel.superstep_seq, st.pregel.superstep_seq);
    assert_eq!(back.pregel.pending_write_queue.len(), st.pregel.pending_write_queue.len());
    assert_eq!(
        back.pregel.pending_write_queue[0].task_id,
        st.pregel.pending_write_queue[0].task_id
    );
}

#[test]
fn multi_round_interrupt_resume_updates_last_resume_cursor() {
    let tid = ThreadId::new_v4();
    let mut st = ThreadState::new(tid);

    st.pregel.interrupt = Some(InterruptSnapshot {
        kind: InterruptKind::Clarification { prompt: Some("a".into()) },
        resume_cursor: ResumeCursor {
            node_id: "dispatch_clarify".into(),
            superstep_seq: 1,
            step_seq: StepSeq::initial(),
        },
        payload: json!({}),
    });
    maybe_resume_from_interrupt(&mut st, &[json!("first")]);
    assert!(st.pregel.interrupt.is_none());
    let first = st.pregel.last_resume_at.clone().expect("resume1");

    st.pregel.interrupt = Some(InterruptSnapshot {
        kind: InterruptKind::Clarification { prompt: Some("b".into()) },
        resume_cursor: ResumeCursor {
            node_id: "dispatch_clarify".into(),
            superstep_seq: 2,
            step_seq: StepSeq::initial(),
        },
        payload: json!({}),
    });
    maybe_resume_from_interrupt(&mut st, &[json!("second")]);
    assert!(st.pregel.interrupt.is_none());
    let second = st.pregel.last_resume_at.clone().expect("resume2");
    assert_ne!(first.superstep_seq, second.superstep_seq);
}

#[test]
fn mixed_tool_and_subagent_push_slots_preserve_order() {
    let mut st = ThreadState::new(ThreadId::new_v4());
    superstep_kernel::prepare_tasks(&mut st);
    let calls = vec![ToolCallSpec { name: "t".into(), args: json!({}), call_id: "tc".into() }];
    superstep_kernel::prepare::prepare_tool_fanout(&mut st, &calls);
    superstep_kernel::prepare::prepare_subagent_fanout(&mut st, 2);
    assert_eq!(st.pregel.staged_tasks.len(), 3);
    match &st.pregel.staged_tasks[0].kind {
        TaskKind::Push { fanout_id, call_id } => {
            assert_eq!(fanout_id, "tool_invoke");
            assert_eq!(call_id, "tc");
        }
        other => panic!("expected tool push first: {other:?}"),
    }
}

#[test]
fn subtask_truncation_deterministic_under_same_budget() {
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
    let a = agent_loop_runtime::truncate_subtask_plan(plan.clone(), &budget);
    let b = agent_loop_runtime::truncate_subtask_plan(plan, &budget);
    assert_eq!(a.0.tasks.len(), b.0.tasks.len());
    assert_eq!(a.1, b.1);
}
