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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_loop::AgentLoopState;
    use crate::config::AgentLoopConfig;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    fn test_state() -> AgentLoopState {
        let thread_id = Uuid::new_v4();
        let config = AgentLoopConfig::default();
        AgentLoopState::new(thread_id, "test".to_string(), &config)
    }

    #[test]
    fn test_hook_system_creation() {
        let hooks = HookSystem::new();
        assert!(hooks.hooks.blocking_read().is_empty());
    }

    #[test]
    fn test_hook_system_default() {
        let hooks = HookSystem::default();
        assert!(hooks.hooks.blocking_read().is_empty());
    }

    #[tokio::test]
    async fn test_register_and_run_single_hook() {
        let hooks = HookSystem::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        hooks.register_hook(
            HookPhase::BeforeLoop,
            Box::new(move |_state| {
                let called = called_clone.clone();
                Box::pin(async move {
                    let mut c = called.lock().await;
                    *c = true;
                    Ok(())
                })
            }),
        );

        let state = test_state();
        hooks.run_hooks(HookPhase::BeforeLoop, &state).await.unwrap();

        assert!(*called.lock().await);
    }

    #[tokio::test]
    async fn test_register_and_run_multiple_hooks() {
        let hooks = HookSystem::new();
        let call_count = Arc::new(Mutex::new(0));

        for _ in 0..3 {
            let call_count = call_count.clone();
            hooks.register_hook(
                HookPhase::BeforeIteration,
                Box::new(move |_state| {
                    let call_count = call_count.clone();
                    Box::pin(async move {
                        let mut count = call_count.lock().await;
                        *count += 1;
                        Ok(())
                    })
                }),
            );
        }

        let state = test_state();
        hooks.run_hooks(HookPhase::BeforeIteration, &state).await.unwrap();

        assert_eq!(*call_count.lock().await, 3);
    }

    #[tokio::test]
    async fn test_hooks_run_in_correct_phase() {
        let hooks = HookSystem::new();
        let before_loop_called = Arc::new(Mutex::new(false));
        let after_loop_called = Arc::new(Mutex::new(false));

        let blc = before_loop_called.clone();
        hooks.register_hook(
            HookPhase::BeforeLoop,
            Box::new(move |_state| {
                let blc = blc.clone();
                Box::pin(async move {
                    let mut c = blc.lock().await;
                    *c = true;
                    Ok(())
                })
            }),
        );

        let alc = after_loop_called.clone();
        hooks.register_hook(
            HookPhase::AfterCompletion,
            Box::new(move |_state| {
                let alc = alc.clone();
                Box::pin(async move {
                    let mut c = alc.lock().await;
                    *c = true;
                    Ok(())
                })
            }),
        );

        let state = test_state();
        hooks.run_hooks(HookPhase::BeforeLoop, &state).await.unwrap();

        assert!(*before_loop_called.lock().await);
        assert!(!(*after_loop_called.lock().await));
    }

    #[tokio::test]
    async fn test_hook_count() {
        let hooks = HookSystem::new();

        assert_eq!(hooks.hook_count().await, 0);

        hooks.register_hook(HookPhase::BeforeLoop, Box::new(|_state| Box::pin(async { Ok(()) })));
        assert_eq!(hooks.hook_count().await, 1);

        hooks.register_hook(
            HookPhase::BeforeIteration,
            Box::new(|_state| Box::pin(async { Ok(()) })),
        );
        hooks.register_hook(
            HookPhase::AfterIteration,
            Box::new(|_state| Box::pin(async { Ok(()) })),
        );
        assert_eq!(hooks.hook_count().await, 3);
    }

    #[tokio::test]
    async fn test_clear_hooks() {
        let mut hooks = HookSystem::new();

        hooks.register_hook(HookPhase::BeforeLoop, Box::new(|_state| Box::pin(async { Ok(()) })));
        hooks.register_hook(
            HookPhase::AfterCompletion,
            Box::new(|_state| Box::pin(async { Ok(()) })),
        );

        assert_eq!(hooks.hook_count().await, 2);

        hooks.clear_hooks();

        assert_eq!(hooks.hook_count().await, 0);
    }

    #[tokio::test]
    async fn test_hooks_receive_state() {
        let hooks = HookSystem::new();
        let received_iteration = Arc::new(Mutex::new(0));

        let ri = received_iteration.clone();
        hooks.register_hook(
            HookPhase::BeforeIteration,
            Box::new(move |state| {
                let ri = ri.clone();
                let iteration = state.iteration;
                Box::pin(async move {
                    let mut r = ri.lock().await;
                    *r = iteration;
                    Ok(())
                })
            }),
        );

        let thread_id = Uuid::new_v4();
        let config = AgentLoopConfig::default();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);
        state.iteration = 42;

        hooks.run_hooks(HookPhase::BeforeIteration, &state).await.unwrap();

        assert_eq!(*received_iteration.lock().await, 42);
    }

    #[tokio::test]
    async fn test_all_hook_phases() {
        let hooks = HookSystem::new();
        let phases_called = Arc::new(Mutex::new(Vec::new()));

        let phases = vec![
            HookPhase::BeforeLoop,
            HookPhase::BeforeIteration,
            HookPhase::AfterIteration,
            HookPhase::BeforeCompletion,
            HookPhase::OnCompletionDetected,
            HookPhase::AfterCompletion,
        ];

        for &phase in &phases {
            let phases_called = phases_called.clone();
            hooks.register_hook(
                phase,
                Box::new(move |_state| {
                    let phases_called = phases_called.clone();
                    Box::pin(async move {
                        let mut p = phases_called.lock().await;
                        p.push(phase);
                        Ok(())
                    })
                }),
            );
        }

        let state = test_state();

        for &phase in &phases {
            hooks.run_hooks(phase, &state).await.unwrap();
        }

        let called = phases_called.lock().await.clone();
        assert_eq!(called.len(), 6);
    }

    #[tokio::test]
    async fn test_hook_error_propagation() {
        let hooks = HookSystem::new();

        hooks.register_hook(
            HookPhase::BeforeLoop,
            Box::new(|_state| Box::pin(async { Err(anyhow::anyhow!("hook failed")) })),
        );

        let state = test_state();
        let result = hooks.run_hooks(HookPhase::BeforeLoop, &state).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hook failed"));
    }
}
