//! Integration tests for budget tracking.

use agent_loop_runtime::RunBudget;
use agent_loop_runtime::{BudgetTracker, ConcurrencyGuard};
use agent_ports::{RunId, ThreadId};
use std::time::Duration;

#[test]
fn test_budget_tracker_creation() {
    let budget = RunBudget::default();
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    assert_eq!(tracker.tokens_used(), 0);
    assert_eq!(tracker.concurrent_tasks(), 0);
    assert!(!tracker.is_time_budget_exceeded());
    assert!(!tracker.is_token_budget_exceeded());
}

#[test]
fn test_token_tracking() {
    let budget = RunBudget { token_budget: Some(1000), ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    tracker.add_tokens(500);
    assert_eq!(tracker.tokens_used(), 500);
    assert!(!tracker.is_token_budget_exceeded());

    tracker.add_tokens(600);
    assert_eq!(tracker.tokens_used(), 1100);
    assert!(tracker.is_token_budget_exceeded());
}

#[test]
fn test_remaining_tokens() {
    let budget = RunBudget { token_budget: Some(1000), ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    assert_eq!(tracker.remaining_tokens(), Some(1000));

    tracker.add_tokens(300);
    assert_eq!(tracker.remaining_tokens(), Some(700));

    tracker.add_tokens(800);
    assert_eq!(tracker.remaining_tokens(), Some(0));
}

#[test]
fn test_retry_tracking() {
    let budget = RunBudget { max_retries_per_task: 3, ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    let task_id = "task-1";
    assert_eq!(tracker.get_retry_count(task_id), 0);
    assert!(!tracker.is_retry_budget_exceeded(task_id));

    tracker.increment_retry(task_id);
    tracker.increment_retry(task_id);
    assert_eq!(tracker.get_retry_count(task_id), 2);
    assert!(!tracker.is_retry_budget_exceeded(task_id));

    tracker.increment_retry(task_id);
    assert_eq!(tracker.get_retry_count(task_id), 3);
    assert!(tracker.is_retry_budget_exceeded(task_id));
}

#[test]
fn test_concurrency_tracking() {
    let budget = RunBudget { max_concurrent_subagents: 4, ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    assert_eq!(tracker.concurrent_tasks(), 0);
    assert!(!tracker.is_concurrency_limit_exceeded());

    tracker.start_task();
    tracker.start_task();
    assert_eq!(tracker.concurrent_tasks(), 2);
    assert!(!tracker.is_concurrency_limit_exceeded());

    tracker.start_task();
    tracker.start_task();
    assert_eq!(tracker.concurrent_tasks(), 4);
    assert!(tracker.is_concurrency_limit_exceeded());

    tracker.complete_task();
    assert_eq!(tracker.concurrent_tasks(), 3);
    assert!(!tracker.is_concurrency_limit_exceeded());
}

#[test]
fn test_concurrency_guard() {
    let budget = RunBudget { max_concurrent_subagents: 2, ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    assert_eq!(tracker.concurrent_tasks(), 0);

    {
        let guard1 = ConcurrencyGuard::try_acquire(&tracker).expect("Should acquire first guard");
        assert_eq!(tracker.concurrent_tasks(), 1);

        {
            let guard2 =
                ConcurrencyGuard::try_acquire(&tracker).expect("Should acquire second guard");
            assert_eq!(tracker.concurrent_tasks(), 2);

            // Third should fail
            assert!(ConcurrencyGuard::try_acquire(&tracker).is_none());
            assert_eq!(tracker.concurrent_tasks(), 2);

            drop(guard2);
        }

        assert_eq!(tracker.concurrent_tasks(), 1);
        drop(guard1);
    }

    assert_eq!(tracker.concurrent_tasks(), 0);
}

#[test]
fn test_utilization_summary() {
    let budget = RunBudget {
        max_total_wall_time: Some(Duration::from_secs(60)),
        token_budget: Some(1000),
        max_concurrent_subagents: 4,
        ..Default::default()
    };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    let summary = tracker.utilization_summary();

    assert!(summary.time.is_some());
    assert!(summary.tokens.is_some());
    assert_eq!(summary.concurrency, 0.0);
    assert!(summary.time.expect("time should be some") >= 0.0);
    assert!(summary.tokens.expect("tokens should be some") == 0.0);
}

#[test]
fn test_budget_utilization_threshold() {
    let utilization = agent_loop_runtime::BudgetUtilization {
        time: Some(0.5),
        tokens: Some(0.8),
        concurrency: 0.3,
    };

    assert!(utilization.is_above_threshold(0.4));
    assert!(!utilization.is_above_threshold(0.9));
    assert_eq!(utilization.max_utilization(), 0.8);
}

#[tokio::test]
async fn test_time_budget_exceeded() {
    let budget =
        RunBudget { max_total_wall_time: Some(Duration::from_millis(100)), ..Default::default() };
    let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), budget);

    assert!(!tracker.is_time_budget_exceeded());

    tokio::time::sleep(Duration::from_millis(150)).await;

    assert!(tracker.is_time_budget_exceeded());
    assert!(tracker.remaining_wall_time() == Some(Duration::ZERO));
}
