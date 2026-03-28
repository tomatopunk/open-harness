//! Per-run inputs from governance + request (middleware hints).

use agent_ports::TodoItem;
use protocol_compat::Configurable;

/// Knobs that align the inner loop with lead-pipeline middleware and governance.
#[derive(Debug, Clone)]
pub struct AgentLoopRunConfig {
    /// LangGraph `configurable` (same knobs as lead / DeerFlow-style request).
    pub configurable: Configurable,
    pub model_name: Option<String>,
    pub policy_version: String,
    pub is_plan_mode: bool,
    pub skills_globally_enabled: bool,
    /// Skill names to activate (from governance entries ∩ this list).
    pub enabled_skill_names: Vec<String>,
    pub loop_detected: bool,
    /// Seed todos when plan mode or middleware produced steps.
    pub seed_todos: Vec<TodoItem>,
}

impl Default for AgentLoopRunConfig {
    fn default() -> Self {
        Self {
            configurable: Configurable::default(),
            model_name: None,
            policy_version: String::new(),
            is_plan_mode: false,
            skills_globally_enabled: true,
            enabled_skill_names: Vec::new(),
            loop_detected: false,
            seed_todos: Vec::new(),
        }
    }
}
