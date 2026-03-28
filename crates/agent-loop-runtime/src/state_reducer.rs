//! Pure state evolution helpers (reducer-style) for [`agent_ports::ThreadState`].

use crate::run_config::AgentLoopRunConfig;
use agent_ports::{ChatMessage, ThreadState};

/// One-time bootstrap from run configuration (policy, todos, plan mode, loop tag).
pub fn apply_run_config_bootstrap(state: &mut ThreadState, run_cfg: &AgentLoopRunConfig) {
    if !run_cfg.policy_version.is_empty() {
        state.governance_marks.policy_version = Some(run_cfg.policy_version.clone());
    }
    if run_cfg.loop_detected {
        state.governance_marks.tags.push("loop_detected".into());
    }
    for t in &run_cfg.seed_todos {
        state.todos.push(t.clone());
    }
    if run_cfg.is_plan_mode {
        state.plan_state.active = true;
    }
}

/// Append user messages to the conversation transcript.
pub fn append_user_messages(state: &mut ThreadState, user_messages: &[serde_json::Value]) {
    for msg in user_messages {
        state.messages.push(ChatMessage { role: "user".into(), content: msg.clone() });
    }
}

/// Merge subagent task records after a plan completes (append-only reducer).
pub fn append_subagent_task_records(
    state: &mut ThreadState,
    records: Vec<agent_ports::SubagentTaskRecord>,
) {
    state.subagent_tasks.extend(records);
}

/// Append a single assistant message (text path).
pub fn append_assistant_text(state: &mut ThreadState, text: String) {
    state
        .messages
        .push(ChatMessage { role: "assistant".into(), content: serde_json::Value::String(text) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::{ThreadId, TodoItem};

    #[test]
    fn bootstrap_sets_plan_and_policy() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        let cfg = AgentLoopRunConfig {
            policy_version: "p1".into(),
            is_plan_mode: true,
            seed_todos: vec![TodoItem { id: "1".into(), title: "t".into(), done: false }],
            ..Default::default()
        };
        apply_run_config_bootstrap(&mut st, &cfg);
        assert_eq!(st.governance_marks.policy_version.as_deref(), Some("p1"));
        assert!(st.plan_state.active);
        assert_eq!(st.todos.len(), 1);
    }
}
