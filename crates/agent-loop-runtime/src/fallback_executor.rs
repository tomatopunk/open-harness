//! Fallback strategy executor for handling unrecoverable failures.
//!
//! This module provides mechanisms to gracefully handle failures that cannot
//! be resolved through retries, including simplified task execution and
//! default value fallbacks.

use agent_ports::{FallbackStrategy, PortResult, SubtaskSpec, TaskFailure};
use serde_json::Value;

/// Executor for fallback strategies when retries are exhausted.
pub struct FallbackExecutor {
    strategy: FallbackStrategy,
}

impl FallbackExecutor {
    /// Create a new fallback executor with the given strategy.
    #[must_use]
    pub fn new(strategy: FallbackStrategy) -> Self {
        Self { strategy }
    }

    /// Create with default strategy (SkipAndContinue).
    #[must_use]
    pub fn with_default_strategy() -> Self {
        Self::new(FallbackStrategy::default())
    }

    /// Execute the fallback strategy for a failed task.
    ///
    /// # Arguments
    /// * `failed_task` - The task that failed
    /// * `failure` - The failure that occurred
    /// * `simplified_executor` - Optional executor for simplified tasks
    ///
    /// # Returns
    /// * `FallbackResult` - The outcome of the fallback strategy
    pub async fn execute<F, Fut>(
        &self,
        failed_task: &SubtaskSpec,
        failure: &TaskFailure,
        simplified_executor: Option<F>,
    ) -> FallbackResult
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = PortResult<Value>>,
    {
        match &self.strategy {
            FallbackStrategy::SkipAndContinue => {
                FallbackResult::Skipped {
                    task_goal: failed_task.goal.clone(),
                    failure: failure.clone(),
                }
            }

            FallbackStrategy::UseDefault(value) => FallbackResult::UsedDefault {
                value: value.clone(),
                original_goal: failed_task.goal.clone(),
            },

            FallbackStrategy::UseSimplified { task_goal, task_input: _ } => {
                if let Some(executor) = simplified_executor {
                    match executor().await {
                        Ok(result) => FallbackResult::SimplifiedSuccess {
                            result,
                            original_goal: failed_task.goal.clone(),
                            simplified_goal: task_goal.clone(),
                        },
                        Err(e) => FallbackResult::SimplifiedFailed {
                            original_goal: failed_task.goal.clone(),
                            simplified_goal: task_goal.clone(),
                            error: e.to_string(),
                        },
                    }
                } else {
                    FallbackResult::NoSimplifiedExecutor {
                        task_goal: task_goal.clone(),
                    }
                }
            }

            FallbackStrategy::FailFast => FallbackResult::FailedFast {
                task_goal: failed_task.goal.clone(),
                failure: failure.clone(),
            },

            FallbackStrategy::RetryDegraded { max_retries, reduced_timeout_ms } => {
                // This is handled at a higher level - return degraded retry request
                FallbackResult::RequestDegradedRetry {
                    max_retries: *max_retries,
                    reduced_timeout_ms: *reduced_timeout_ms,
                    task_goal: failed_task.goal.clone(),
                }
            }
        }
    }

    /// Get the strategy for inspection.
    #[must_use]
    pub fn strategy(&self) -> &FallbackStrategy {
        &self.strategy
    }

    /// Check if this failure should trigger fallback.
    #[must_use]
    pub fn should_fallback(&self, failure: &TaskFailure) -> bool {
        match failure {
            TaskFailure::Permanent { .. } => true,
            TaskFailure::BudgetExceeded { .. } => true,
            TaskFailure::Transient { retryable, .. } => !retryable,
            TaskFailure::Timeout { .. } => false, // Timeouts usually retry first
            TaskFailure::System { retryable, .. } => !retryable,
        }
    }
}

/// Result of executing a fallback strategy.
#[derive(Debug, Clone)]
pub enum FallbackResult {
    /// Task was skipped, continue with other tasks.
    Skipped {
        task_goal: String,
        failure: TaskFailure,
    },

    /// Used a default value.
    UsedDefault {
        value: Value,
        original_goal: String,
    },

    /// Successfully executed simplified task.
    SimplifiedSuccess {
        result: Value,
        original_goal: String,
        simplified_goal: String,
    },

    /// Failed to execute simplified task.
    SimplifiedFailed {
        original_goal: String,
        simplified_goal: String,
        error: String,
    },

    /// No simplified executor was provided.
    NoSimplifiedExecutor {
        task_goal: String,
    },

    /// Immediately terminated.
    FailedFast {
        task_goal: String,
        failure: TaskFailure,
    },

    /// Request degraded retry (to be handled by parent).
    RequestDegradedRetry {
        max_retries: u32,
        reduced_timeout_ms: Option<u64>,
        task_goal: String,
    },
}

impl FallbackResult {
    /// Check if this result represents a successful outcome.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            Self::UsedDefault { .. } | Self::SimplifiedSuccess { .. }
        )
    }

    /// Extract the result value if available.
    #[must_use]
    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::UsedDefault { value, .. } => Some(value),
            Self::SimplifiedSuccess { result, .. } => Some(result),
            _ => None,
        }
    }

    /// Get a human-readable description of the fallback outcome.
    #[must_use]
    pub fn description(&self) -> String {
        match self {
            Self::Skipped { task_goal, .. } => {
                format!("Skipped task: {task_goal}")
            }
            Self::UsedDefault { original_goal, .. } => {
                format!("Used default value for: {original_goal}")
            }
            Self::SimplifiedSuccess { original_goal, simplified_goal, .. } => {
                format!("Simplified '{simplified_goal}' succeeded for: {original_goal}")
            }
            Self::SimplifiedFailed { original_goal, simplified_goal, error } => {
                format!("Simplified '{simplified_goal}' failed for {original_goal}: {error}")
            }
            Self::NoSimplifiedExecutor { task_goal } => {
                format!("No simplified executor for: {task_goal}")
            }
            Self::FailedFast { task_goal, .. } => {
                format!("Failed fast on task: {task_goal}")
            }
            Self::RequestDegradedRetry { task_goal, .. } => {
                format!("Requesting degraded retry for: {task_goal}")
            }
        }
    }
}

/// Helper to create common fallback strategies.
pub mod fallback_presets {
    use super::*;
    use serde_json::json;

    /// Create a fallback that returns an empty result.
    #[must_use]
    #[allow(dead_code)]
    pub fn empty_result() -> FallbackStrategy {
        FallbackStrategy::UseDefault(json!({ "fallback": "empty_result" }))
    }

    /// Create a fallback that returns a custom message.
    #[must_use]
    #[allow(dead_code)]
    pub fn custom_message(message: &str) -> FallbackStrategy {
        FallbackStrategy::UseDefault(json!({ "fallback": "message", "content": message }))
    }

    /// Create a fallback that skips and continues.
    #[must_use]
    #[allow(dead_code)]
    pub fn skip_and_continue() -> FallbackStrategy {
        FallbackStrategy::SkipAndContinue
    }

    /// Create a fail-fast fallback.
    #[must_use]
    #[allow(dead_code)]
    pub fn fail_fast() -> FallbackStrategy {
        FallbackStrategy::FailFast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_task() -> SubtaskSpec {
        SubtaskSpec {
            goal: "Test task".to_string(),
            input: Value::Null,
            budget_steps: 5,
        }
    }

    #[tokio::test]
    async fn test_skip_and_continue() {
        let executor = FallbackExecutor::with_default_strategy();
        let task = create_test_task();
        let failure = TaskFailure::permanent("test error");

        let result = executor
            .execute(&task, &failure, None::<fn() -> _>)
            .await;

        assert!(matches!(result, FallbackResult::Skipped { .. }));
        assert!(result.is_success() == false);
    }

    #[tokio::test]
    async fn test_use_default() {
        let default_value = json!({ "status": "default" });
        let executor = FallbackExecutor::new(FallbackStrategy::UseDefault(default_value.clone()));
        let task = create_test_task();
        let failure = TaskFailure::permanent("test error");

        let result = executor
            .execute(&task, &failure, None::<fn() -> _>)
            .await;

        assert!(matches!(result, FallbackResult::UsedDefault { .. }));
        assert!(result.is_success());
        assert_eq!(result.into_value(), Some(default_value));
    }

    #[tokio::test]
    async fn test_simplified_success() {
        let executor = FallbackExecutor::new(FallbackStrategy::UseSimplified {
            task_goal: "Simplified task".to_string(),
            task_input: Value::Null,
        });
        let task = create_test_task();
        let failure = TaskFailure::permanent("test error");

        let result = executor
            .execute(&task, &failure, Some(|| async { Ok(json!({ "status": "simplified" })) }))
            .await;

        assert!(matches!(result, FallbackResult::SimplifiedSuccess { .. }));
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_fail_fast() {
        let executor = FallbackExecutor::new(FallbackStrategy::FailFast);
        let task = create_test_task();
        let failure = TaskFailure::permanent("test error");

        let result = executor
            .execute(&task, &failure, None::<fn() -> _>)
            .await;

        assert!(matches!(result, FallbackResult::FailedFast { .. }));
        assert!(!result.is_success());
    }

    #[test]
    fn test_fallback_presets() {
        let _empty = fallback_presets::empty_result();
        let _message = fallback_presets::custom_message("test");
        let _skip = fallback_presets::skip_and_continue();
        let _fail = fallback_presets::fail_fast();
    }
}
