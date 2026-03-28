//! DeerFlow-style robustness: dangling tool transcript repair, loop detection on tool patterns.
//!
//! Observability is intentionally out of scope; behavior changes are deterministic.

use agent_ports::{ChatMessage, LlmTurnOutput, ThreadState, ToolCallSpec};
use serde_json::json;
use serde_json::Value;
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::hash::{Hash, Hasher};

/// If invocations exist without a matching result row (e.g. interrupted turn), append synthetic error results and tool messages.
pub(crate) fn repair_missing_tool_results(state: &mut ThreadState) {
    let mut pending: Vec<String> = Vec::new();
    for inv in &state.tool_invocations {
        let has = state.tool_results.iter().any(|r| r.invocation_id == inv.invocation_id);
        if !has {
            pending.push(inv.invocation_id.clone());
        }
    }
    for id in pending {
        if let Some(inv) = state.tool_invocations.iter().find(|i| i.invocation_id == id).cloned() {
            state.tool_results.push(agent_ports::ToolResultRecord {
                invocation_id: id.clone(),
                tool_name: inv.tool_name.clone(),
                ok: false,
                payload: json!({ "error": "synthetic_repair: missing tool result for invocation" }),
            });
            state.messages.push(ChatMessage {
                role: "tool".into(),
                content: json!({
                    "tool_call_id": id,
                    "name": inv.tool_name,
                    "content": { "error": "synthetic_repair: missing tool result for invocation" }
                }),
            });
        }
    }
}

#[must_use]
fn canonical_args_key(args: &Value) -> String {
    serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
}

/// Multiset of (tool name, args JSON) — aligns with roadmap / DeerFlow-style loop detection.
#[must_use]
fn fingerprint_tool_calls(calls: &[ToolCallSpec]) -> u64 {
    let mut counts: BTreeMap<(String, String), u32> = BTreeMap::new();
    for c in calls {
        let key = (c.name.clone(), canonical_args_key(&c.args));
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut h = DefaultHasher::new();
    for (k, n) in counts {
        k.0.hash(&mut h);
        k.1.hash(&mut h);
        n.hash(&mut h);
    }
    h.finish()
}

/// If the model repeats the same tool-call fingerprint as the previous non-empty turn, strip tools and emit a short assistant hint (loop breaker).
pub(crate) fn apply_repeated_tool_loop_breaker(
    out: &mut LlmTurnOutput,
    last_fingerprint: &mut Option<u64>,
) {
    if out.tool_calls.is_empty() {
        *last_fingerprint = None;
        return;
    }
    let fp = fingerprint_tool_calls(&out.tool_calls);
    if last_fingerprint.as_ref() == Some(&fp) {
        out.assistant_text = Some(
            "Loop breaker: repeated identical tool calls were suppressed; respond with a summary or a different approach."
                .into(),
        );
        out.tool_calls.clear();
        out.subtask_plan = None;
        out.needs_clarification = false;
        out.finish_turn = true;
    }
    *last_fingerprint = Some(fp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::ThreadId;
    use serde_json::json;

    #[test]
    fn repair_synthesizes_missing_result() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        st.tool_invocations.push(agent_ports::ToolInvocationRecord {
            tool_name: "echo".into(),
            args: json!({}),
            invocation_id: "i1".into(),
        });
        repair_missing_tool_results(&mut st);
        assert_eq!(st.tool_results.len(), 1);
        assert!(!st.tool_results[0].ok);
    }

    #[test]
    fn repeated_identical_tool_calls_suppressed_on_second_turn() {
        let calls = vec![agent_ports::ToolCallSpec {
            name: "echo".into(),
            args: json!({ "x": 1 }),
            call_id: "c1".into(),
        }];
        let mut out1 = LlmTurnOutput { tool_calls: calls.clone(), ..Default::default() };
        let mut last_fp: Option<u64> = None;
        apply_repeated_tool_loop_breaker(&mut out1, &mut last_fp);
        assert!(!out1.tool_calls.is_empty());

        let mut out2 = LlmTurnOutput { tool_calls: calls.clone(), ..Default::default() };
        apply_repeated_tool_loop_breaker(&mut out2, &mut last_fp);
        assert!(out2.tool_calls.is_empty());
        assert!(out2.finish_turn);
        assert!(out2.assistant_text.is_some());
    }
}
