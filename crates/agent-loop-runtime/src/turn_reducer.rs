//! Turn-scoped state updates as a small effect list (reducer-style), used by the inner loop.

use agent_ports::{ChatMessage, SubagentTaskRecord, ThreadState};
use serde_json::{json, Value};

/// Pure state transitions applied after model dispatch (and related paths).
#[derive(Debug, Clone)]
pub enum TurnEffect {
    /// Set clarification flags from model output.
    SetClarification { prompt: Option<String> },
    /// Append-only subagent task records.
    AppendSubagentRecords(Vec<SubagentTaskRecord>),
    /// Replace thread state after subagent merge (authoritative snapshot).
    ReplaceFromMerge(Box<ThreadState>),
    /// Append one assistant message (string content).
    AppendAssistantMessage(String),
    /// Record a full tool round: invocations, results, and synthetic tool role messages.
    ApplyToolRound {
        invocations: Vec<agent_ports::ToolInvocationRecord>,
        results: Vec<agent_ports::ToolResultRecord>,
        tool_messages: Vec<ChatMessage>,
    },
}

/// Apply a batch of effects in order (consumes so [`TurnEffect::ReplaceFromMerge`] can move).
pub fn apply_turn_effects(state: &mut ThreadState, effects: Vec<TurnEffect>) {
    for e in effects {
        match e {
            TurnEffect::SetClarification { prompt } => {
                state.clarification_state.pending = true;
                state.clarification_state.prompt = prompt;
            }
            TurnEffect::AppendSubagentRecords(records) => {
                state.subagent_tasks.extend(records);
            }
            TurnEffect::ReplaceFromMerge(merged) => {
                *state = *merged;
            }
            TurnEffect::AppendAssistantMessage(text) => {
                state
                    .messages
                    .push(ChatMessage { role: "assistant".into(), content: Value::String(text) });
            }
            TurnEffect::ApplyToolRound { invocations, results, tool_messages } => {
                state.tool_invocations.extend(invocations);
                state.tool_results.extend(results);
                state.messages.extend(tool_messages);
            }
        }
    }
}

/// Build tool round records from successful/failed invokes (shared with loop dispatch).
#[must_use]
pub fn tool_round_from_calls(
    calls: &[agent_ports::ToolCallSpec],
    payloads: Vec<Result<Value, String>>,
) -> TurnEffect {
    let mut invocations = Vec::with_capacity(calls.len());
    let mut results = Vec::with_capacity(calls.len());
    let mut tool_messages = Vec::with_capacity(calls.len());

    for (call, payload_res) in calls.iter().zip(payloads) {
        let ok = payload_res.is_ok();
        let payload = payload_res.unwrap_or_else(|e| json!({ "error": e }));
        invocations.push(agent_ports::ToolInvocationRecord {
            tool_name: call.name.clone(),
            args: call.args.clone(),
            invocation_id: call.call_id.clone(),
        });
        results.push(agent_ports::ToolResultRecord {
            invocation_id: call.call_id.clone(),
            tool_name: call.name.clone(),
            ok,
            payload: payload.clone(),
        });
        tool_messages.push(ChatMessage {
            role: "tool".into(),
            content: json!({
                "tool_call_id": call.call_id,
                "name": call.name,
                "content": payload
            }),
        });
    }

    TurnEffect::ApplyToolRound { invocations, results, tool_messages }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::ThreadId;

    #[test]
    fn clarification_effect_sets_flags() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        apply_turn_effects(
            &mut st,
            vec![TurnEffect::SetClarification { prompt: Some("x".into()) }],
        );
        assert!(st.clarification_state.pending);
        assert_eq!(st.clarification_state.prompt.as_deref(), Some("x"));
    }

    #[test]
    fn tool_round_effect_appends_all() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        let call = agent_ports::ToolCallSpec {
            name: "echo".into(),
            args: json!({}),
            call_id: "c1".into(),
        };
        let e = tool_round_from_calls(std::slice::from_ref(&call), vec![Ok(json!("ok"))]);
        apply_turn_effects(&mut st, vec![e]);
        assert_eq!(st.tool_invocations.len(), 1);
        assert_eq!(st.tool_results.len(), 1);
        assert_eq!(st.messages.len(), 1);
    }
}
