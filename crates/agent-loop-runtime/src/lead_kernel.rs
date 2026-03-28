//! Per-turn lead [`runtime_kernel::RuntimeKernel`] merge into [`agent_ports::ThreadState`].
//!
//! Aligns the inner loop with the same middleware chain as [`orchestrator_core::LeadPipeline`]
//! (DeerFlow-style `before_turn`), so policy is not only applied once at the HTTP boundary.

use agent_ports::{ThreadState, TodoItem};
use runtime_kernel::{MiddlewareContext, RuntimeKernel};
use std::sync::Arc;

use crate::error::{AgentLoopError, AgentLoopResult};
use crate::run_config::AgentLoopRunConfig;

/// Run the shared lead kernel on the current transcript and merge results into `state`.
pub(crate) async fn apply_lead_kernel_turn(
    kernel: Arc<RuntimeKernel>,
    state: &mut ThreadState,
    run_cfg: &AgentLoopRunConfig,
) -> AgentLoopResult<()> {
    if state.messages.is_empty() {
        return Ok(());
    }
    let msgs: Vec<serde_json::Value> = state.messages.iter().map(|m| m.content.clone()).collect();
    let mut cfg = run_cfg.configurable.clone();
    if run_cfg.model_name.is_some() {
        cfg.model_name = run_cfg.model_name.clone();
    }
    if run_cfg.is_plan_mode {
        cfg.is_plan_mode = Some(true);
    }
    let ctx = kernel
        .prepare_with_input(cfg, msgs)
        .await
        .map_err(|e| AgentLoopError::LeadKernel(e.to_string()))?;
    merge_middleware_into_state(state, &ctx);
    Ok(())
}

fn merge_middleware_into_state(state: &mut ThreadState, ctx: &MiddlewareContext) {
    if ctx.loop_detected && !state.governance_marks.tags.iter().any(|t| t == "loop_detected") {
        state.governance_marks.tags.push("loop_detected".into());
    }
    for title in &ctx.todos {
        if !state.todos.iter().any(|t| t.title == *title) {
            let id = format!("lead-{}", state.todos.len());
            state.todos.push(TodoItem { id, title: title.clone(), done: false });
        }
    }
    for fact in &ctx.memory_facts {
        if !state.memory_working_set.snippets.iter().any(|s| s == fact) {
            state.memory_working_set.snippets.push(fact.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::ThreadId;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn lead_kernel_merges_loop_tag_and_snippets() {
        let kernel = RuntimeKernel::default();
        let tid = ThreadId::new_v4();
        let mut state = ThreadState::new(tid);
        state
            .messages
            .push(agent_ports::ChatMessage { role: "user".into(), content: json!("repeat") });
        state
            .messages
            .push(agent_ports::ChatMessage { role: "user".into(), content: json!("repeat") });
        let run_cfg = AgentLoopRunConfig::default();
        apply_lead_kernel_turn(Arc::new(kernel), &mut state, &run_cfg).await.expect("kernel");
        assert!(state.governance_marks.tags.iter().any(|t| t == "loop_detected"));
    }
}
