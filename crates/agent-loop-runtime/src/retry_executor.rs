//! Retry executor with intelligent backoff and failure classification.
//!
//! This module provides automatic retry logic with exponential backoff,
//! jitter, and intelligent failure classification.

use agent_ports::{RetryPolicy, TaskFailure};
use std::future::Future;
use std::time::Duration;

/// Executor that handles retry logic for fallible operations.
pub struct RetryExecutor {
    policy: RetryPolicy,
}

impl RetryExecutor {
    /// Create a new retry executor with the given policy.
    #[must_use]
    pub fn new(policy: RetryPolicy) -> Self {
        Self { policy }
    }

    /// Create with default policy.
    #[must_use]
    pub fn with_default_policy() -> Self {
        Self::new(RetryPolicy::default())
    }

    /// Execute a fallible operation with retry logic.
    ///
    /// # Type Parameters
    /// * `F` - The future type returned by the operation
    /// * `T` - The success type
    /// * `E` - The error type (must be convertible to TaskFailure)
    ///
    /// # Arguments
    /// * `operation` - The async operation to execute
    /// * `classify_error` - Function to classify errors for retry decisions
    ///
    /// # Returns
    /// * `Ok(T)` - The operation succeeded
    /// * `Err(TaskFailure)` - The operation failed after all retries
    pub async fn execute<F, Fut, T, E>(
        &self,
        mut operation: F,
        mut classify_error: impl FnMut(&E) -> TaskFailure,
    ) -> Result<T, TaskFailure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut attempt = 0;

        loop {
            attempt += 1;

            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    let failure = classify_error(&error);

                    // Check if we should retry
                    if !self.policy.should_retry(&failure) || attempt >= self.policy.max_retries {
                        return Err(failure);
                    }

                    // Calculate and apply delay
                    let delay_ms = self.policy.calculate_delay(attempt);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    /// Execute with a simple error classification (all errors are transient).
    pub async fn execute_simple<F, Fut, T, E>(&self, operation: F) -> Result<T, TaskFailure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        self.execute(operation, |e| TaskFailure::transient(e.to_string(), None)).await
    }

    /// Get the policy for inspection.
    #[must_use]
    pub fn policy(&self) -> &RetryPolicy {
        &self.policy
    }
}

/// Helper function to classify common error types.
#[must_use]
pub fn classify_common_error<E: std::fmt::Display>(error: &E) -> TaskFailure {
    let error_str = error.to_string().to_lowercase();

    // Check for transient errors
    if error_str.contains("timeout")
        || error_str.contains("temporarily")
        || error_str.contains("rate limit")
        || error_str.contains("unavailable")
        || error_str.contains("connection")
    {
        return TaskFailure::transient(error.to_string(), None);
    }

    // Check for permanent errors
    if error_str.contains("not found")
        || error_str.contains("invalid")
        || error_str.contains("unauthorized")
        || error_str.contains("forbidden")
    {
        return TaskFailure::permanent(error.to_string());
    }

    // Default to transient for unknown errors
    TaskFailure::transient(error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests are in tests/retry_fallback_integration.rs
    // Unit tests with closures have complex lifetime requirements

    #[test]
    fn test_retry_policy_creation() {
        let policy = RetryPolicy::default()
            .with_max_retries(3)
            .with_initial_delay(1000)
            .with_max_delay(30000)
            .with_jitter(true);

        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.initial_delay_ms, 1000);
        assert!(policy.jitter);
    }

    #[test]
    fn test_retry_delay_calculation() {
        let policy = RetryPolicy::default()
            .with_initial_delay(1000)
            .with_backoff_factor(2.0)
            .with_jitter(false);

        assert_eq!(policy.calculate_delay(1), 1000);
        assert_eq!(policy.calculate_delay(2), 2000);
        assert_eq!(policy.calculate_delay(3), 4000);
    }
}
