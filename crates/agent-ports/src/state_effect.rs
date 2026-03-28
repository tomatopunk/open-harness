//! Serializable state transitions for checkpoint replay (Pregel-style pending writes).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::thread_state::{
    ChatMessage, SubagentTaskRecord, ThreadState, ToolInvocationRecord, ToolResultRecord,
};

/// Pure state transitions applied after model dispatch (tool rounds, clarification, merge, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum StateEffect {
    SetClarification {
        prompt: Option<String>,
    },
    AppendSubagentRecords {
        records: Vec<SubagentTaskRecord>,
    },
    ReplaceFromMerge {
        state: Box<ThreadState>,
    },
    AppendAssistantMessage {
        text: String,
    },
    ApplyToolRound {
        invocations: Vec<ToolInvocationRecord>,
        results: Vec<ToolResultRecord>,
        tool_messages: Vec<ChatMessage>,
    },
}

/// Apply a batch of effects in order.
pub fn apply_state_effects(out: &mut ThreadState, effects: &[StateEffect]) {
    for e in effects {
        match e {
            StateEffect::SetClarification { prompt } => {
                out.clarification_state.pending = true;
                out.clarification_state.prompt = prompt.clone();
            }
            StateEffect::AppendSubagentRecords { records } => {
                out.subagent_tasks.extend(records.iter().cloned());
            }
            StateEffect::ReplaceFromMerge { state } => {
                *out = state.as_ref().clone();
            }
            StateEffect::AppendAssistantMessage { text } => {
                out.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: Value::String(text.clone()),
                });
            }
            StateEffect::ApplyToolRound { invocations, results, tool_messages } => {
                out.tool_invocations.extend(invocations.iter().cloned());
                out.tool_results.extend(results.iter().cloned());
                out.messages.extend(tool_messages.iter().cloned());
            }
        }
    }
}

/// Build tool round records from successful/failed invokes.
#[must_use]
pub fn tool_round_from_calls(
    calls: &[crate::ToolCallSpec],
    payloads: Vec<Result<Value, String>>,
) -> StateEffect {
    let mut invocations = Vec::with_capacity(calls.len());
    let mut results = Vec::with_capacity(calls.len());
    let mut tool_messages = Vec::with_capacity(calls.len());

    for (call, payload_res) in calls.iter().zip(payloads) {
        let ok = payload_res.is_ok();
        let payload = payload_res.unwrap_or_else(|e| json!({ "error": e }));
        invocations.push(ToolInvocationRecord {
            tool_name: call.name.clone(),
            args: call.args.clone(),
            invocation_id: call.call_id.clone(),
        });
        results.push(ToolResultRecord {
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

    StateEffect::ApplyToolRound { invocations, results, tool_messages }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThreadId;

    #[test]
    fn clarification_effect_sets_flags() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        apply_state_effects(&mut st, &[StateEffect::SetClarification { prompt: Some("x".into()) }]);
        assert!(st.clarification_state.pending);
        assert_eq!(st.clarification_state.prompt.as_deref(), Some("x"));
    }
}
