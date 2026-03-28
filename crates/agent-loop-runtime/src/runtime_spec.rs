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

impl Default for LeadRuntimeSpec {
    fn default() -> Self {
        Self { apply_lead_kernel: true, premodel_skills_memory: true }
    }
}

/// Subagent branch placeholder for future subagent-only knobs (middleware tiers, tool masks).
#[derive(Debug, Clone, Default)]
pub struct SubagentRuntimeSpec {}
