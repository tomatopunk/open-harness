//! Task-driven superstep scheduling (LangGraph-style `prepare_tasks` → `execute_tasks` → `apply_writes`).
//!
//! - **prepare**: record [`TaskEnvelope`] rows in [`agent_ports::PregelMeta::staged_tasks`].
//! - **execute**: [`crate::dispatch::execute_dispatch_plan`] (tools, subagents, text/memory, interrupt).
//! - **apply_writes**: [`crate::pregel::bump_after_node`] records channel versions + [`PregelMeta::pending_write_queue`].

use agent_ports::{TaskEnvelope, ThreadState, ToolCallSpec};
use uuid::Uuid;

/// Stage PULL tasks for named phase nodes (lead, premodel, model, postmodel, …).
/// Returns the task envelope id for pairing with [`crate::pregel::bump_after_node`].
#[inline]
#[must_use]
pub fn prepare_pull_task(state: &mut ThreadState, node_id: &str) -> Uuid {
    let env = TaskEnvelope::pull(node_id);
    let id = env.id;
    state.pregel.staged_tasks.push(env);
    id
}

/// First [`TaskEnvelope::id`] among the last `n` staged tasks (for PUSH batch `apply_writes` pairing).
#[must_use]
pub fn first_task_id_in_staged_tail(state: &ThreadState, n: usize) -> Option<Uuid> {
    if n == 0 {
        return None;
    }
    let len = state.pregel.staged_tasks.len();
    if len < n {
        return None;
    }
    let start = len - n;
    state.pregel.staged_tasks.get(start).map(|e| e.id)
}

/// Stage PUSH tasks for concurrent tool invokes (one envelope per allowed call).
pub fn prepare_tool_fanout(state: &mut ThreadState, calls: &[ToolCallSpec]) {
    for call in calls {
        state
            .pregel
            .staged_tasks
            .push(TaskEnvelope::push_with_call_id("tool_invoke", call.call_id.clone()));
    }
}

/// Stage PUSH tasks for subagent plan slots.
pub fn prepare_subagent_fanout(state: &mut ThreadState, task_count: usize) {
    for i in 0..task_count {
        state
            .pregel
            .staged_tasks
            .push(TaskEnvelope::push_with_call_id("subagent_task", format!("subagent:{i}")));
    }
}

/// Reset staged tasks at the beginning of an outer superstep (one `run_agent_loop` iteration).
///
/// Hosts and tests should prefer [`crate::superstep_kernel::prepare_tasks`].
#[inline]
pub(crate) fn begin_outer_superstep(state: &mut ThreadState) {
    state.pregel.staged_tasks.clear();
}
