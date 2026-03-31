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

        // Run before loop hooks
        let state_guard = self.state.read().await;
        self.hooks.run_hooks(HookPhase::BeforeLoop, &state_guard).await?;
        drop(state_guard);

        // Start the main loop
        self.run_loop().await?;

        Ok(())
    }

    /// Stop the current loop.
    pub async fn stop_loop(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.active = false;

        let state_guard = self.state.read().await;
        self.hooks.run_hooks(HookPhase::AfterCompletion, &state_guard).await?;

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
        // Before iteration hooks
        let state_guard = self.state.read().await;
        self.hooks.run_hooks(HookPhase::BeforeIteration, &state_guard).await?;

        // Check if we should stop
        if state_guard.should_stop() {
            self.stop_loop().await?;
            return Ok(false);
        }
        drop(state_guard);

        // Increment iteration
        {
            let mut state = self.state.write().await;
            state.increment_iteration();
        }

        // After iteration hooks (the actual work happens in hooks)
        let state_guard = self.state.read().await;
        self.hooks.run_hooks(HookPhase::AfterIteration, &state_guard).await?;

        // Check for completion
        {
            let _state = self.state.read().await;
            // Completion detection would happen here after getting transcript
            // For now, we leave it to the hooks to detect and stop
        }

        self.hooks.run_hooks(HookPhase::BeforeCompletion, &state_guard).await?;

        Ok(true)
    }

    /// Called when completion is detected.
    pub async fn on_completion_detected(&self) -> Result<()> {
        let state_guard = self.state.read().await;
        self.hooks.run_hooks(HookPhase::OnCompletionDetected, &state_guard).await?;
        self.stop_loop().await?;
        Ok(())
    }
}
