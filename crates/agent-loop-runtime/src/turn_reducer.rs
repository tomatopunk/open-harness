//! Turn-scoped state updates (re-export of [`agent_ports::StateEffect`]).

pub use agent_ports::{
    apply_state_effects as apply_turn_effects, tool_round_from_calls, StateEffect as TurnEffect,
};

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::ThreadId;
    use serde_json::json;

    #[test]
    fn clarification_effect_sets_flags() {
        let tid = ThreadId::new_v4();
        let mut st = agent_ports::ThreadState::new(tid);
        apply_turn_effects(&mut st, &[TurnEffect::SetClarification { prompt: Some("x".into()) }]);
        assert!(st.clarification_state.pending);
        assert_eq!(st.clarification_state.prompt.as_deref(), Some("x"));
    }

    #[test]
    fn tool_round_effect_appends_all() {
        let tid = ThreadId::new_v4();
        let mut st = agent_ports::ThreadState::new(tid);
        let call = agent_ports::ToolCallSpec {
            name: "echo".into(),
            args: json!({}),
            call_id: "c1".into(),
        };
        let e = tool_round_from_calls(std::slice::from_ref(&call), vec![Ok(json!("ok"))]);
        apply_turn_effects(&mut st, &[e]);
        assert_eq!(st.tool_invocations.len(), 1);
        assert_eq!(st.tool_results.len(), 1);
        assert_eq!(st.messages.len(), 1);
    }
}
