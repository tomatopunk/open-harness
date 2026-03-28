//! Lead vs subagent runtime templates (DeerFlow-style dual builder) as explicit specs.
//!
//! Shared middleware stays on [`crate::middleware::AgentLoopMiddleware`]; these structs only
//! gate which phases run so Lead and Subagent paths stay aligned under one scheduler.

/// Phases before the model call (lead kernel + PreModel skill/memory).
#[derive(Debug, Clone)]
pub struct LeadRuntimeSpec {
    /// When true, merge [`runtime_kernel::RuntimeKernel`] output into [`agent_ports::ThreadState`].
    pub apply_lead_kernel: bool,
    /// When true, run skill injection + memory retrieve during PreModel.
    pub premodel_skills_memory: bool,
}

impl LeadRuntimeSpec {
    /// Pregel phase node id for the lead kernel step (must match scheduler pulls).
    pub const NODE_LEAD: &'static str = "lead";
    /// PreModel skill/memory injection phase.
    pub const NODE_PREMODEL: &'static str = "premodel";
    /// LLM inference phase.
    pub const NODE_MODEL: &'static str = "model";
    /// Post-model middleware phase.
    pub const NODE_POSTMODEL: &'static str = "postmodel";

    /// Fixed order of PULL phase nodes for one outer superstep (contract tests anchor here).
    #[must_use]
    pub fn main_phase_pull_order() -> &'static [&'static str] {
        &[Self::NODE_LEAD, Self::NODE_PREMODEL, Self::NODE_MODEL, Self::NODE_POSTMODEL]
    }
}

impl Default for LeadRuntimeSpec {
    fn default() -> Self {
        Self { apply_lead_kernel: true, premodel_skills_memory: true }
    }
}

/// Subagent branch: knobs that differ from [`LeadRuntimeSpec`] for delegated runs (ChildRun / P5).
#[derive(Debug, Clone, Default)]
pub struct SubagentRuntimeSpec {
    /// When true, subagent runs use the same PreModel skill/memory path as the lead template.
    pub inherit_premodel_skills_memory: bool,
}
