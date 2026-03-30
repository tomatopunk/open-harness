//! Integration tests for retry executor and fallback strategies.

use agent_loop_runtime::{FallbackExecutor, FallbackResult, RetryExecutor};
use agent_ports::{FallbackStrategy, RetryPolicy, TaskFailure};
use serde_json::json;

#[tokio::test]
async fn test_retry_executor_success() {
    let executor = RetryExecutor::new(
        RetryPolicy::default().with_max_retries(3).with_initial_delay(10).with_jitter(false),
    );

    let attempts = std::sync::Arc::new(std::sync::Mutex::new(0));
    let result = executor
        .execute_simple({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    let mut guard = attempts.lock().expect("Failed to acquire lock");
                    *guard += 1;
                    if *guard < 2 {
                        Err("temporary error")
                    } else {
                        Ok("success")
                    }
                }
            }
        })
        .await;

    assert!(result.is_ok());
    assert_eq!(*attempts.lock().expect("Failed to acquire lock"), 2);
    assert_eq!(result.expect("Result should be Ok"), "success");
}

#[tokio::test]
async fn test_retry_executor_exhausts_retries() {
    let executor = RetryExecutor::new(
        RetryPolicy::default().with_max_retries(2).with_initial_delay(10).with_jitter(false),
    );

    let attempts = std::sync::Arc::new(std::sync::Mutex::new(0));
    let result = executor
        .execute_simple({
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    let mut guard = attempts.lock().expect("Failed to acquire lock");
                    *guard += 1;
                    Err::<String, _>("persistent error")
                }
            }
        })
        .await;

    assert!(result.is_err());
    assert_eq!(*attempts.lock().expect("Failed to acquire lock"), 2); // max_retries = 2 means 2 attempts
}

#[tokio::test]
async fn test_retry_executor_permanent_error() {
    let executor = RetryExecutor::with_default_policy();

    let attempts = std::sync::Arc::new(std::sync::Mutex::new(0));
    let result = executor
        .execute(
            {
                let attempts = attempts.clone();
                move || {
                    let attempts = attempts.clone();
                    async move {
                        let mut guard = attempts.lock().expect("Failed to acquire lock");
                        *guard += 1;
                        Err::<String, _>("invalid request")
                    }
                }
            },
            |_| TaskFailure::permanent("invalid request"),
        )
        .await;

    assert!(result.is_err());
    assert_eq!(*attempts.lock().expect("Failed to acquire lock"), 1); // No retries for permanent errors
}

#[tokio::test]
async fn test_fallback_skip_and_continue() {
    let executor = FallbackExecutor::with_default_strategy();
    let task = agent_ports::SubtaskSpec {
        goal: "Test task".to_string(),
        input: json!(null),
        budget_steps: 5,
    };
    let failure = TaskFailure::permanent("test error");

    let result = executor
        .execute(
            &task,
            &failure,
            Option::<
                fn() -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = agent_ports::PortResult<serde_json::Value>>
                            + Send,
                    >,
                >,
            >::None,
        )
        .await;

    assert!(matches!(result, FallbackResult::Skipped { .. }));
    assert!(!result.is_success());
}

#[tokio::test]
async fn test_fallback_use_default() {
    let default_value = json!({ "status": "default" });
    let executor = FallbackExecutor::new(FallbackStrategy::UseDefault(default_value.clone()));
    let task = agent_ports::SubtaskSpec {
        goal: "Test task".to_string(),
        input: json!(null),
        budget_steps: 5,
    };
    let failure = TaskFailure::permanent("test error");

    let result = executor
        .execute(
            &task,
            &failure,
            Option::<
                fn() -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = agent_ports::PortResult<serde_json::Value>>
                            + Send,
                    >,
                >,
            >::None,
        )
        .await;

    assert!(matches!(result, FallbackResult::UsedDefault { .. }));
    assert!(result.is_success());
    assert_eq!(result.into_value(), Some(default_value));
}

#[tokio::test]
async fn test_fallback_simplified_task() {
    let executor = FallbackExecutor::new(FallbackStrategy::UseSimplified {
        task_goal: "Simplified task".to_string(),
        task_input: json!(null),
    });
    let task = agent_ports::SubtaskSpec {
        goal: "Original task".to_string(),
        input: json!(null),
        budget_steps: 5,
    };
    let failure = TaskFailure::permanent("test error");

    let result = executor
        .execute(&task, &failure, Some(|| async { Ok(json!({ "status": "simplified" })) }))
        .await;

    assert!(matches!(result, FallbackResult::SimplifiedSuccess { .. }));
    assert!(result.is_success());
}

#[tokio::test]
async fn test_fallback_fail_fast() {
    let executor = FallbackExecutor::new(FallbackStrategy::FailFast);
    let task = agent_ports::SubtaskSpec {
        goal: "Test task".to_string(),
        input: json!(null),
        budget_steps: 5,
    };
    let failure = TaskFailure::permanent("test error");

    let result = executor
        .execute(
            &task,
            &failure,
            Option::<
                fn() -> std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = agent_ports::PortResult<serde_json::Value>>
                            + Send,
                    >,
                >,
            >::None,
        )
        .await;

    assert!(matches!(result, FallbackResult::FailedFast { .. }));
    assert!(!result.is_success());
}

#[test]
fn test_fallback_result_description() {
    let skipped = FallbackResult::Skipped {
        task_goal: "Test".to_string(),
        failure: TaskFailure::permanent("error"),
    };
    assert!(skipped.description().contains("Skipped"));

    let default_result =
        FallbackResult::UsedDefault { value: json!(null), original_goal: "Test".to_string() };
    assert!(default_result.description().contains("default"));
}
