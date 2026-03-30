//! Port-level errors (transport-agnostic).

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortError {
    #[error("llm: {0}")]
    Llm(String),
    #[error("tool: {0}")]
    Tool(String),
    #[error("memory: {0}")]
    Memory(String),
    #[error("skill: {0}")]
    Skill(String),
    #[error("subagent: {0}")]
    Subagent(String),
    #[error("checkpoint: {0}")]
    Checkpoint(String),
    #[error("thread_state: {0}")]
    ThreadState(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error("budget_exhausted: {0}")]
    BudgetExhausted(String),
    #[error("clarification_required")]
    ClarificationRequired,
    #[error("aborted: {0}")]
    Aborted(String),
}

pub type PortResult<T> = Result<T, PortError>;

/// Task failure classification for intelligent retry logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFailure {
    /// Transient errors that may succeed on retry (network timeouts, temporary failures)
    Transient {
        reason: String,
        retryable: bool,
        suggested_delay_ms: Option<u64>,
    },
    /// Permanent errors that should not be retried (invalid parameters, tool not found)
    Permanent { reason: String },
    /// Budget exceeded (time, tokens, retry count)
    BudgetExceeded {
        budget_type: BudgetType,
        current: u64,
        limit: u64,
    },
    /// Timeout errors (task took too long)
    Timeout { elapsed_ms: u64, timeout_ms: u64 },
    /// System errors (internal bugs, invariant violations)
    System { reason: String, retryable: bool },
}

/// Type of budget that was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetType {
    Time,
    Tokens,
    Retries,
    ConcurrentTasks,
    SubagentTasks,
}

impl TaskFailure {
    /// Check if this failure type should trigger a retry.
    #[must_use]
    pub fn should_retry(&self) -> bool {
        match self {
            Self::Transient { retryable, .. } => *retryable,
            Self::Permanent { .. } => false,
            Self::BudgetExceeded { .. } => false,
            Self::Timeout { .. } => true,
            Self::System { retryable, .. } => *retryable,
        }
    }

    /// Get the suggested retry delay if available.
    #[must_use]
    pub fn suggested_delay(&self) -> Option<u64> {
        match self {
            Self::Transient { suggested_delay_ms, .. } => *suggested_delay_ms,
            _ => None,
        }
    }

    /// Create a transient error with optional suggested delay.
    #[must_use]
    pub fn transient(reason: impl Into<String>, suggested_delay_ms: Option<u64>) -> Self {
        Self::Transient {
            reason: reason.into(),
            retryable: true,
            suggested_delay_ms,
        }
    }

    /// Create a permanent error.
    #[must_use]
    pub fn permanent(reason: impl Into<String>) -> Self {
        Self::Permanent { reason: reason.into() }
    }

    /// Create a budget exceeded error.
    #[must_use]
    pub fn budget_exceeded(budget_type: BudgetType, current: u64, limit: u64) -> Self {
        Self::BudgetExceeded { budget_type, current, limit }
    }
}

/// Retry policy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay for backoff in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum delay for backoff in milliseconds
    pub max_delay_ms: u64,
    /// Exponential backoff factor (e.g., 2.0 means delay doubles each retry)
    pub backoff_factor: f64,
    /// Whether to add random jitter to delays
    pub jitter: bool,
    /// Which failure types should trigger retries
    pub retry_on: Vec<TaskFailure>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_factor: 2.0,
            jitter: true,
            retry_on: vec![
                TaskFailure::Transient { reason: String::new(), retryable: true, suggested_delay_ms: None },
                TaskFailure::Timeout { elapsed_ms: 0, timeout_ms: 0 },
            ],
        }
    }
}

impl RetryPolicy {
    /// Create a new retry policy with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum number of retries.
    #[must_use]
    pub fn with_max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Set the initial delay in milliseconds.
    #[must_use]
    pub fn with_initial_delay(mut self, delay_ms: u64) -> Self {
        self.initial_delay_ms = delay_ms;
        self
    }

    /// Set the maximum delay in milliseconds.
    #[must_use]
    pub fn with_max_delay(mut self, delay_ms: u64) -> Self {
        self.max_delay_ms = delay_ms;
        self
    }

    /// Set the backoff factor.
    #[must_use]
    pub fn with_backoff_factor(mut self, factor: f64) -> Self {
        self.backoff_factor = factor;
        self
    }

    /// Enable or disable jitter.
    #[must_use]
    pub fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// Check if a specific failure should be retried based on this policy.
    #[must_use]
    pub fn should_retry(&self, failure: &TaskFailure) -> bool {
        if !failure.should_retry() {
            return false;
        }

        // Check if this failure type is in our retry list
        self.retry_on.iter().any(|retry_failure| {
            std::mem::discriminant(retry_failure) == std::mem::discriminant(failure)
        })
    }

    /// Calculate the delay for a given attempt number (in milliseconds).
    #[must_use]
    pub fn calculate_delay(&self, attempt: u32) -> u64 {
        let base_delay = self.initial_delay_ms as f64;
        let backoff = self.backoff_factor;
        let max_delay = self.max_delay_ms as f64;

        // Exponential backoff: base_delay * (backoff ^ (attempt - 1))
        let delay = base_delay * backoff.powi(attempt as i32 - 1);
        let delay = if delay > max_delay { max_delay } else { delay };

        // Add jitter if enabled
        if self.jitter {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            // Add ±20% jitter
            let jitter_range = (delay * 0.2) as u64;
            let jitter = rng.gen_range(0..jitter_range * 2) as f64 - jitter_range as f64;
            (delay + jitter).max(0.0) as u64
        } else {
            delay as u64
        }
    }
}

/// Fallback strategy for handling unrecoverable failures.
#[derive(Debug, Clone)]
pub enum FallbackStrategy {
    /// Skip the failed task and continue with others
    SkipAndContinue,
    /// Use a default value
    UseDefault(serde_json::Value),
    /// Fall back to a simplified version of the task
    UseSimplified { task_goal: String, task_input: serde_json::Value },
    /// Immediately terminate and report failure
    FailFast,
    /// Retry with reduced expectations (e.g., fewer tools, shorter timeout)
    RetryDegraded { max_retries: u32, reduced_timeout_ms: Option<u64> },
}

impl Default for FallbackStrategy {
    fn default() -> Self {
        Self::SkipAndContinue
    }
}

// Convert TaskFailure to PortError for easier error handling
impl From<TaskFailure> for PortError {
    fn from(failure: TaskFailure) -> Self {
        match failure {
            TaskFailure::Transient { reason, .. } | TaskFailure::Permanent { reason } | TaskFailure::System { reason, .. } => {
                PortError::Subagent(reason)
            }
            TaskFailure::Timeout { elapsed_ms, timeout_ms } => {
                PortError::Subagent(format!("Task timeout: {elapsed_ms}ms exceeded {timeout_ms}ms"))
            }
            TaskFailure::BudgetExceeded { budget_type, current, limit } => {
                let budget_type_str = match budget_type {
                    BudgetType::Time => "time",
                    BudgetType::Tokens => "tokens",
                    BudgetType::Retries => "retries",
                    BudgetType::ConcurrentTasks => "concurrent tasks",
                    BudgetType::SubagentTasks => "subagent tasks",
                };
                PortError::BudgetExhausted(format!(
                    "{budget_type_str} budget exceeded: {current}/{limit}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        // Transient errors
        assert!(error_helpers::is_transient_error("Connection timeout"));
        assert!(error_helpers::is_transient_error("Rate limit exceeded"));
        assert!(error_helpers::is_transient_error("Network unavailable"));

        // Permanent errors
        assert!(error_helpers::is_permanent_error("Resource not found"));
        assert!(error_helpers::is_permanent_error("Invalid parameter"));
        assert!(error_helpers::is_permanent_error("Unauthorized access"));
    }

    #[test]
    fn test_classify_error_msg() {
        let transient = error_helpers::classify_error_msg("Connection timeout");
        assert!(matches!(transient, TaskFailure::Transient { .. }));

        let permanent = error_helpers::classify_error_msg("Invalid request");
        assert!(matches!(permanent, TaskFailure::Permanent { .. }));
    }

    #[test]
    fn test_timeout_failure() {
        let failure = error_helpers::timeout_failure(5000, 3000);
        assert!(matches!(failure, TaskFailure::Timeout { elapsed_ms: 5000, timeout_ms: 3000 }));
    }

    #[test]
    fn test_budget_exceeded_failure() {
        let failure = error_helpers::budget_exceeded_failure(BudgetType::Tokens, 150000, 100000);
        assert!(matches!(failure, TaskFailure::BudgetExceeded { .. }));
    }

    #[test]
    fn test_task_failure_to_port_error() {
        let transient = TaskFailure::transient("test error", None);
        let port_error: PortError = transient.into();
        assert!(matches!(port_error, PortError::Subagent(_)));

        let timeout = TaskFailure::Timeout { elapsed_ms: 5000, timeout_ms: 3000 };
        let port_error: PortError = timeout.into();
        assert!(matches!(port_error, PortError::Subagent(_)));
    }
}

/// Helper functions for error classification and handling.
pub mod error_helpers {
    use super::*;

    /// Classify an error string as transient or permanent based on common patterns.
    #[must_use]
    pub fn classify_error_msg(error: &str) -> TaskFailure {
        let lower = error.to_lowercase();

        // Transient errors
        if lower.contains("timeout")
            || lower.contains("temporarily")
            || lower.contains("rate limit")
            || lower.contains("unavailable")
            || lower.contains("connection")
            || lower.contains("network")
        {
            return TaskFailure::transient(error.to_string(), None);
        }

        // Permanent errors
        if lower.contains("not found")
            || lower.contains("invalid")
            || lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("bad request")
            || lower.contains("unprocessable")
        {
            return TaskFailure::permanent(error.to_string());
        }

        // Default to transient for unknown errors
        TaskFailure::transient(error.to_string(), None)
    }

    /// Check if an error message suggests a transient failure.
    #[must_use]
    pub fn is_transient_error(error: &str) -> bool {
        let lower = error.to_lowercase();
        lower.contains("timeout")
            || lower.contains("temporarily")
            || lower.contains("rate limit")
            || lower.contains("unavailable")
            || lower.contains("connection")
            || lower.contains("network")
    }

    /// Check if an error message suggests a permanent failure.
    #[must_use]
    pub fn is_permanent_error(error: &str) -> bool {
        let lower = error.to_lowercase();
        lower.contains("not found")
            || lower.contains("invalid")
            || lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("bad request")
    }

    /// Create a timeout failure.
    #[must_use]
    pub fn timeout_failure(elapsed_ms: u64, timeout_ms: u64) -> TaskFailure {
        TaskFailure::Timeout { elapsed_ms, timeout_ms }
    }

    /// Create a budget exceeded failure.
    #[must_use]
    pub fn budget_exceeded_failure(budget_type: BudgetType, current: u64, limit: u64) -> TaskFailure {
        TaskFailure::budget_exceeded(budget_type, current, limit)
    }

    /// Wrap a result with error classification.
    pub fn classify_result<T>(result: Result<T, impl std::fmt::Display>) -> Result<T, TaskFailure> {
        result.map_err(|e| classify_error_msg(&e.to_string()))
    }
}
