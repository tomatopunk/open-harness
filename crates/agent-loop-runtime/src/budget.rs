//! Unified budgets for subagents and turns (governance-driven).

#[derive(Debug, Clone, Copy)]
pub struct RunBudget {
    pub max_turns: u32,
    pub max_subagent_tasks: u32,
    pub max_concurrent_subagents: u32,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self { max_turns: 16, max_subagent_tasks: 8, max_concurrent_subagents: 4 }
    }
}
