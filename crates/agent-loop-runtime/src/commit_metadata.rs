//! Keys and builders for [`graph_runtime_core::GraphRuntime::commit_step`] metadata JSON.
//!
//! Keep payloads consistent across branches so hosts can map errors and inspect checkpoints.

/// JSON object keys for checkpoint metadata attached to each `commit_step`.
pub mod keys {
    pub const STAGE: &str = "stage";
    pub const REASON: &str = "reason";
    pub const TOOL_NAMES: &str = "tool_names";
    pub const SUBAGENT_TASK_COUNT: &str = "subagent_task_count";
    pub const SUBAGENT_PLAN_TRUNCATED: &str = "subagent_plan_truncated";
}

use serde_json::{json, Value};

#[must_use]
pub fn clarify_exit() -> Value {
    json!({
        keys::STAGE: "clarify_exit",
        keys::REASON: "clarification",
    })
}

#[must_use]
pub fn state_commit_after_subagent(task_count: usize, plan_truncated: bool) -> Value {
    json!({
        keys::STAGE: "state_commit",
        keys::REASON: "subagent_plan_complete",
        keys::SUBAGENT_TASK_COUNT: task_count,
        keys::SUBAGENT_PLAN_TRUNCATED: plan_truncated,
    })
}

#[must_use]
pub fn state_commit_after_tools(tool_names: &[String]) -> Value {
    json!({
        keys::STAGE: "state_commit",
        keys::REASON: "tool_round_complete",
        keys::TOOL_NAMES: tool_names,
    })
}

#[must_use]
pub fn state_commit_after_memory_turn() -> Value {
    json!({
        keys::STAGE: "state_commit",
        keys::REASON: "memory_and_transcript_turn",
    })
}
