//! Task-driven superstep scheduling (LangGraph-style `prepare_tasks` → `execute_tasks` → `apply_writes`).
//!
//! - **prepare**: record [`TaskEnvelope`] rows in [`agent_ports::PregelMeta::staged_tasks`].
//! - **execute**: performed by `engine_v2` / `dispatch` (LLM, tools, subagents).
//! - **apply_writes**: [`crate::pregel::bump_after_node`] records channel versions + [`PregelMeta::pending_write_queue`].

use agent_ports::{TaskEnvelope, ThreadState, ToolCallSpec};

/// Stage PULL tasks for named phase nodes (lead, premodel, model, postmodel, …).
#[inline]
pub fn prepare_pull_task(state: &mut ThreadState, node_id: &str) {
    state.pregel.staged_tasks.push(TaskEnvelope::pull(node_id));
}

/// Stage PUSH tasks for concurrent tool invokes (one envelope per allowed call).
pub fn prepare_tool_fanout(state: &mut ThreadState, calls: &[ToolCallSpec]) {
    for (i, _call) in calls.iter().enumerate() {
        state.pregel.staged_tasks.push(TaskEnvelope::push("tool_invoke", i as u32));
    }
}

/// Stage PUSH tasks for subagent plan slots.
pub fn prepare_subagent_fanout(state: &mut ThreadState, task_count: usize) {
    for i in 0..task_count {
        state.pregel.staged_tasks.push(TaskEnvelope::push("subagent_task", i as u32));
    }
}

/// Reset staged tasks at the beginning of an outer superstep (one `run_agent_loop` iteration).
#[inline]
pub fn begin_outer_superstep(state: &mut ThreadState) {
    state.pregel.staged_tasks.clear();
}
