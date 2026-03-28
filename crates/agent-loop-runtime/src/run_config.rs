//! Per-run inputs from governance + request (middleware hints).

use agent_ports::{BuildDispatchPlanOptions, ProviderStrategy, TodoItem, ToolStrategy};
use protocol_compat::Configurable;

use crate::runtime_spec::{LeadRuntimeSpec, SubagentRuntimeSpec};

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
    /// Lead-oriented phase gates (shared baseline vs subagent-only overrides at call sites).
    pub lead_spec: LeadRuntimeSpec,
    pub subagent_spec: SubagentRuntimeSpec,
    /// Command IR / structured-output strategy (P3); wired into [`agent_ports::build_dispatch_plan_with_options`].
    pub provider_strategy: ProviderStrategy,
    pub tool_strategy: ToolStrategy,
}

impl AgentLoopRunConfig {
    #[must_use]
    pub fn dispatch_plan_options(&self) -> BuildDispatchPlanOptions {
        BuildDispatchPlanOptions {
            provider: self.provider_strategy.clone(),
            tool: self.tool_strategy.clone(),
        }
    }
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
            lead_spec: LeadRuntimeSpec::default(),
            subagent_spec: SubagentRuntimeSpec::default(),
            provider_strategy: ProviderStrategy::default(),
            tool_strategy: ToolStrategy::default(),
        }
    }
}
