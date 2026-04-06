//! Agent Loop implementation - Ralph Loop style inspired by oh-my-openagent.
//!
//! Provides a self-referential agent loop that continues execution until
//! a completion promise is detected in the output.

use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{OnceCell, RwLock};
use uuid::Uuid;

use crate::config::AgentLoopConfig;
use crate::events::EventBus;
use crate::hooks::{HookPhase, HookSystem};
use llm_providers::LLMProvider;

/// State of the Agent Loop.
#[derive(Debug, Clone)]
pub struct AgentLoopState {
    /// Whether the loop is currently active
    pub active: bool,
    /// Current iteration count
    pub iteration: usize,
    /// Maximum iterations
    pub max_iterations: usize,
    /// Completion promise keyword (e.g., "DONE")
    pub completion_promise: String,
    /// Start time
    pub started_at: Instant,
    /// Original prompt
    pub prompt: String,
    /// Thread ID
    pub thread_id: Uuid,
    /// Whether this is ultrawork mode (with verification phase)
    pub ultrawork: bool,
    /// Whether verification is pending
    pub verification_pending: bool,
}

impl AgentLoopState {
    /// Create a new agent loop state.
    pub fn new(thread_id: Uuid, prompt: String, config: &AgentLoopConfig) -> Self {
        Self {
            active: true,
            iteration: 0,
            max_iterations: config.max_iterations,
            completion_promise: config.completion_promise.clone(),
            started_at: Instant::now(),
            prompt,
            thread_id,
            ultrawork: false,
            verification_pending: false,
        }
    }

    /// Check if completion promise is found in transcript.
    pub fn detect_completion(&self, transcript: &str) -> bool {
        // Look for <promise>COMPLETION</promise> pattern
        let pattern = format!("<promise>{}</promise>", self.completion_promise);
        transcript.contains(&pattern) || transcript.contains(&self.completion_promise)
    }

    /// Increment iteration counter.
    pub fn increment_iteration(&mut self) {
        self.iteration += 1;
    }

    /// Check if we've exceeded max iterations.
    pub fn should_stop(&self) -> bool {
        !self.active || self.iteration >= self.max_iterations
    }
}

/// Agent Loop inspired by Ralph Loop from oh-my-openagent.
pub struct AgentLoop {
    config: AgentLoopConfig,
    state: Arc<RwLock<AgentLoopState>>,
    hooks: Arc<HookSystem>,
    event_bus: Arc<EventBus>,
    llm_provider: Arc<OnceCell<Box<dyn LLMProvider>>>,
}

impl AgentLoop {
    /// Create a new Agent Loop.
    pub fn new(
        config: AgentLoopConfig,
        hooks: Arc<HookSystem>,
        event_bus: Arc<EventBus>,
        llm_provider: Arc<OnceCell<Box<dyn LLMProvider>>>,
    ) -> Self {
        // Create a dummy initial state (will be replaced when loop starts)
        let initial_state = AgentLoopState {
            active: false,
            iteration: 0,
            max_iterations: config.max_iterations,
            completion_promise: config.completion_promise.clone(),
            started_at: Instant::now(),
            prompt: String::new(),
            thread_id: Uuid::nil(),
            ultrawork: false,
            verification_pending: false,
        };

        Self { config, state: Arc::new(RwLock::new(initial_state)), hooks, event_bus, llm_provider }
    }

    /// Start a new agent loop.
    pub async fn start_loop(&self, thread_id: Uuid, prompt: String) -> Result<()> {
        let mut state = self.state.write().await;
        *state = AgentLoopState::new(thread_id, prompt, &self.config);
        drop(state);

        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::BeforeLoop, &state_snapshot).await?;

        // Start the main loop
        self.run_loop().await?;

        Ok(())
    }

    /// Stop the current loop.
    pub async fn stop_loop(&self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            state.active = false;
        }

        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::AfterCompletion, &state_snapshot).await?;

        Ok(())
    }

    /// Get current state.
    pub async fn current_state(&self) -> AgentLoopState {
        self.state.read().await.clone()
    }

    /// Check if loop is active.
    pub async fn is_active(&self) -> bool {
        self.state.read().await.active
    }

    /// Run the main loop.
    async fn run_loop(&self) -> Result<()> {
        loop {
            let should_continue = self.run_iteration().await?;
            if !should_continue {
                break;
            }

            // Debounce before next iteration
            tokio::time::sleep(tokio::time::Duration::from_secs(self.config.debounce_seconds))
                .await;
        }

        Ok(())
    }

    /// Run a single iteration.
    async fn run_iteration(&self) -> Result<bool> {
        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::BeforeIteration, &state_snapshot).await?;

        if state_snapshot.should_stop() {
            self.stop_loop().await?;
            return Ok(false);
        }

        {
            let mut state = self.state.write().await;
            state.increment_iteration();
        }

        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::AfterIteration, &state_snapshot).await?;

        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::BeforeCompletion, &state_snapshot).await?;

        Ok(true)
    }

    /// Called when completion is detected.
    pub async fn on_completion_detected(&self) -> Result<()> {
        let state_snapshot = self.state.read().await.clone();
        self.hooks.run_hooks(HookPhase::OnCompletionDetected, &state_snapshot).await?;
        self.stop_loop().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentLoopConfig;
    use crate::events::EventBus;
    use crate::hooks::HookSystem;
    use std::sync::Arc;
    use tokio::sync::OnceCell;
    use uuid::Uuid;

    fn test_config() -> AgentLoopConfig {
        AgentLoopConfig {
            enabled: true,
            max_iterations: 10,
            completion_promise: "DONE".to_string(),
            debounce_seconds: 0,
        }
    }

    #[test]
    fn test_agent_loop_state_creation() {
        let thread_id = Uuid::new_v4();
        let prompt = "test prompt".to_string();
        let config = test_config();

        let state = AgentLoopState::new(thread_id, prompt.clone(), &config);

        assert!(state.active);
        assert_eq!(state.iteration, 0);
        assert_eq!(state.max_iterations, 10);
        assert_eq!(state.completion_promise, "DONE");
        assert_eq!(state.prompt, prompt);
        assert_eq!(state.thread_id, thread_id);
        assert!(!state.ultrawork);
        assert!(!state.verification_pending);
    }

    #[test]
    fn test_agent_loop_state_detect_completion_with_exact_promise() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        assert!(state.detect_completion("some text DONE some text"));
        assert!(!state.detect_completion("no promise here"));
    }

    #[test]
    fn test_agent_loop_state_detect_completion_with_xml_promise() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        assert!(state.detect_completion("some text <promise>DONE</promise> some text"));
    }

    #[test]
    fn test_agent_loop_state_increment_iteration() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        assert_eq!(state.iteration, 0);
        state.increment_iteration();
        assert_eq!(state.iteration, 1);
        state.increment_iteration();
        assert_eq!(state.iteration, 2);
    }

    #[test]
    fn test_agent_loop_state_should_stop_before_max() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        assert!(!state.should_stop());
        state.iteration = 5;
        assert!(!state.should_stop());
    }

    #[test]
    fn test_agent_loop_state_should_stop_at_max() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        state.iteration = 10;
        assert!(state.should_stop());
    }

    #[test]
    fn test_agent_loop_state_should_stop_when_inactive() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        state.active = false;
        assert!(state.should_stop());
    }

    #[tokio::test]
    async fn test_agent_loop_creation() {
        let config = test_config();
        let hooks = Arc::new(HookSystem::new());
        let event_bus = Arc::new(EventBus::new());
        let llm_provider = Arc::new(OnceCell::new());

        let agent_loop = AgentLoop::new(config, hooks, event_bus, llm_provider);

        let state = agent_loop.current_state().await;
        assert!(!state.active);
        assert_eq!(state.iteration, 0);
    }

    #[tokio::test]
    async fn test_agent_loop_start_and_stop() {
        let config = test_config();
        let hooks = Arc::new(HookSystem::new());
        let event_bus = Arc::new(EventBus::new());
        let llm_provider = Arc::new(OnceCell::new());

        let agent_loop = AgentLoop::new(config, hooks, event_bus, llm_provider);
        let thread_id = Uuid::new_v4();

        assert!(!(agent_loop.is_active().await));

        let _ = agent_loop.start_loop(thread_id, "test prompt".to_string()).await;

        let state = agent_loop.current_state().await;
        assert_eq!(state.prompt, "test prompt");
        assert_eq!(state.thread_id, thread_id);

        let _ = agent_loop.stop_loop().await;
        assert!(!(agent_loop.is_active().await));
    }

    #[tokio::test]
    async fn test_agent_loop_current_state() {
        let config = test_config();
        let hooks = Arc::new(HookSystem::new());
        let event_bus = Arc::new(EventBus::new());
        let llm_provider = Arc::new(OnceCell::new());

        let agent_loop = AgentLoop::new(config, hooks, event_bus, llm_provider);

        let state = agent_loop.current_state().await;
        assert!(!state.active);
        assert_eq!(state.iteration, 0);
    }

    #[tokio::test]
    async fn test_agent_loop_is_active() {
        let config = test_config();
        let hooks = Arc::new(HookSystem::new());
        let event_bus = Arc::new(EventBus::new());
        let llm_provider = Arc::new(OnceCell::new());

        let agent_loop = AgentLoop::new(config, hooks, event_bus, llm_provider);

        assert!(!(agent_loop.is_active().await));
    }

    #[test]
    fn test_agent_loop_config_defaults() {
        let config = AgentLoopConfig::default();

        assert!(config.enabled);
        assert_eq!(config.max_iterations, 100);
        assert_eq!(config.completion_promise, "DONE");
        assert_eq!(config.debounce_seconds, 30);
    }

    #[test]
    fn test_agent_loop_state_with_custom_completion_promise() {
        let thread_id = Uuid::new_v4();
        let mut config = test_config();
        config.completion_promise = "FINISHED".to_string();

        let state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        assert!(state.detect_completion("task is FINISHED"));
        assert!(!state.detect_completion("task is DONE"));
    }

    #[test]
    fn test_agent_loop_state_with_ultrawork_mode() {
        let thread_id = Uuid::new_v4();
        let config = test_config();
        let mut state = AgentLoopState::new(thread_id, "test".to_string(), &config);

        state.ultrawork = true;
        state.verification_pending = true;

        assert!(state.ultrawork);
        assert!(state.verification_pending);
    }
}
