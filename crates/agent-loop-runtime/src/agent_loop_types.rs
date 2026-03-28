//! Types shared by the agent loop engine and dispatch.

use crate::middleware::AgentLoopMiddleware;
use agent_ports::{LLMPort, MemoryPort, SkillPort, SubagentPort, ToolAssemblyPolicy, ToolPort};
use runtime_kernel::RuntimeKernel;
use std::sync::Arc;

/// Bundles ports for one agent loop execution.
pub struct AgentLoopDeps {
    pub llm: Arc<dyn LLMPort>,
    pub tools: Arc<dyn ToolPort>,
    pub memory: Arc<dyn MemoryPort>,
    pub skills: Arc<dyn SkillPort>,
    pub subagents: Arc<dyn SubagentPort>,
    /// Same `before_turn` chain as lead runtime (`LeadPipeline` / DeerFlow-style).
    pub lead_kernel: Arc<RuntimeKernel>,
    /// Optional middleware chain (defaults to no-op).
    pub middleware: Arc<dyn AgentLoopMiddleware>,
}

impl std::fmt::Debug for AgentLoopDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoopDeps")
            .field("llm", &"...")
            .field("tools", &"...")
            .field("memory", &"...")
            .field("skills", &"...")
            .field("subagents", &"...")
            .field("lead_kernel", &"...")
            .field("middleware", &"...")
            .finish()
    }
}

impl AgentLoopDeps {
    /// Build deps without custom middleware (no-op).
    #[must_use]
    pub fn new(
        llm: Arc<dyn LLMPort>,
        tools: Arc<dyn ToolPort>,
        memory: Arc<dyn MemoryPort>,
        skills: Arc<dyn SkillPort>,
        subagents: Arc<dyn SubagentPort>,
    ) -> Self {
        Self {
            llm,
            tools,
            memory,
            skills,
            subagents,
            lead_kernel: Arc::new(RuntimeKernel::default()),
            middleware: Arc::new(crate::middleware::NoopMiddleware),
        }
    }
}

/// Configuration for dynamic tool assembly (from governance).
#[derive(Debug, Clone, Default)]
pub struct ToolLoopConfig {
    pub assembly: ToolAssemblyPolicy,
}
