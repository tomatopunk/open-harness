//! Unified budgets for subagents and turns (governance-driven).

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RunBudget {
    pub max_turns: u32,
    pub max_subagent_tasks: u32,
    /// Hard cap on subtasks taken from a single model plan (per response), before `max_subagent_tasks`.
    pub subagent_task_cap_per_response: u32,
    pub max_concurrent_subagents: u32,
    /// Max parallel tool invocations per model turn (DeerFlow / ToolNode-style).
    pub max_concurrent_tool_calls: u32,
    /// Wall-clock limit for each subagent subtask (`None` = no limit).
    pub per_subagent_task_timeout: Option<Duration>,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_turns: 16,
            max_subagent_tasks: 8,
            subagent_task_cap_per_response: 4,
            max_concurrent_subagents: 4,
            max_concurrent_tool_calls: 8,
            per_subagent_task_timeout: Some(Duration::from_secs(120)),
        }
    }
}
