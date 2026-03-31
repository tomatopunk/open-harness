//! Hook system for Agent Loop.
//!
//! Provides a flexible hook system that allows plugins to register callbacks
//! at different phases of the Agent Loop execution.

use anyhow::Result;
use futures::future::BoxFuture;
use std::collections::HashMap;
use tokio::sync::RwLock;

use super::agent_loop::AgentLoopState;

/// Hook phase where the hook will be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPhase {
    /// Before the loop starts
    BeforeLoop,
    /// Before each iteration
    BeforeIteration,
    /// After each iteration
    AfterIteration,
    /// Before completion check
    BeforeCompletion,
    /// After completion is detected
    OnCompletionDetected,
    /// After the loop completes
    AfterCompletion,
}

/// Type alias for hook function.
pub type HookFn = Box<dyn Fn(&AgentLoopState) -> BoxFuture<'static, Result<()>> + Send + Sync>;

/// Hook system manages all registered hooks and runs them at appropriate phases.
pub struct HookSystem {
    hooks: RwLock<HashMap<HookPhase, Vec<HookFn>>>,
}

impl HookSystem {
    /// Create a new empty hook system.
    pub fn new() -> Self {
        Self { hooks: RwLock::new(HashMap::new()) }
    }

    /// Register a hook for a specific phase.
    pub fn register_hook(&self, phase: HookPhase, hook: HookFn) {
        let mut hooks =
            tokio::runtime::Handle::current().block_on(async { self.hooks.write().await });
        hooks.entry(phase).or_default().push(hook);
    }

    /// Run all hooks for a specific phase.
    pub async fn run_hooks(&self, phase: HookPhase, state: &AgentLoopState) -> Result<()> {
        let hooks = self.hooks.read().await;

        if let Some(hooks) = hooks.get(&phase) {
            for hook in hooks {
                hook(state).await?;
            }
        }

        Ok(())
    }

    /// Clear all hooks.
    pub fn clear_hooks(&mut self) {
        let mut hooks =
            tokio::runtime::Handle::current().block_on(async { self.hooks.write().await });
        hooks.clear();
    }

    /// Get number of registered hooks.
    pub async fn hook_count(&self) -> usize {
        self.hooks.read().await.values().map(Vec::len).sum()
    }
}

impl Default for HookSystem {
    fn default() -> Self {
        Self::new()
    }
}
