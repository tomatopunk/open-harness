//! Budget tracking and monitoring for agent runs.
//!
//! This module provides real-time tracking of multiple budget metrics:
//! - Wall-clock time
//! - Token usage
//! - Retry counts
//! - Concurrent task limits

use crate::budget::RunBudget;
use agent_ports::{BudgetType, TaskFailure};
use agent_ports::{RunId, ThreadId};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Tracks budget consumption for a single run.
#[derive(Clone)]
pub struct BudgetTracker {
    inner: Arc<BudgetTrackerInner>,
}

struct BudgetTrackerInner {
    run_id: RunId,
    thread_id: ThreadId,
    budget: RunBudget,
    start_time: Instant,
    tokens_used: AtomicU64,
    retry_counts: RwLock<std::collections::HashMap<String, u32>>,
    concurrent_tasks: AtomicUsize,
}

impl BudgetTracker {
    /// Create a new budget tracker for a run.
    #[must_use]
    pub fn new(run_id: RunId, thread_id: ThreadId, budget: RunBudget) -> Self {
        Self {
            inner: Arc::new(BudgetTrackerInner {
                run_id,
                thread_id,
                budget,
                start_time: Instant::now(),
                tokens_used: AtomicU64::new(0),
                retry_counts: RwLock::new(std::collections::HashMap::new()),
                concurrent_tasks: AtomicUsize::new(0),
            }),
        }
    }

    /// Get the elapsed wall-clock time.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.inner.start_time.elapsed()
    }

    /// Check if wall-clock time budget is exceeded.
    #[must_use]
    pub fn is_time_budget_exceeded(&self) -> bool {
        self.inner.budget.is_wall_time_exceeded(self.elapsed())
    }

    /// Get remaining wall-clock time (None if no limit).
    #[must_use]
    pub fn remaining_wall_time(&self) -> Option<Duration> {
        self.inner.budget.max_total_wall_time.map(|max| {
            let elapsed = self.elapsed();
            if elapsed >= max {
                Duration::ZERO
            } else {
                max - elapsed
            }
        })
    }

    /// Add to token usage.
    pub fn add_tokens(&self, tokens: u64) {
        self.inner.tokens_used.fetch_add(tokens, Ordering::Relaxed);
    }

    /// Get current token usage.
    #[must_use]
    pub fn tokens_used(&self) -> u64 {
        self.inner.tokens_used.load(Ordering::Relaxed)
    }

    /// Check if token budget is exceeded.
    #[must_use]
    pub fn is_token_budget_exceeded(&self) -> bool {
        let used = self.tokens_used();
        self.inner.budget.is_token_budget_exceeded(used)
    }

    /// Get remaining token budget (None if no limit).
    #[must_use]
    pub fn remaining_tokens(&self) -> Option<u64> {
        self.inner.budget.token_budget.map(|max| {
            let used = self.tokens_used();
            max.saturating_sub(used)
        })
    }

    /// Increment retry count for a task.
    pub fn increment_retry(&self, task_id: &str) -> u32 {
        let mut counts =
            self.inner.retry_counts.write().expect("Failed to acquire write lock on retry_counts");
        let count = counts.entry(task_id.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Get retry count for a task.
    #[must_use]
    pub fn get_retry_count(&self, task_id: &str) -> u32 {
        let counts =
            self.inner.retry_counts.read().expect("Failed to acquire read lock on retry_counts");
        *counts.get(task_id).unwrap_or(&0)
    }

    /// Check if retry budget is exceeded for a task.
    #[must_use]
    pub fn is_retry_budget_exceeded(&self, task_id: &str) -> bool {
        let count = self.get_retry_count(task_id);
        count >= self.inner.budget.max_retries_per_task
    }

    /// Increment concurrent task count.
    pub fn start_task(&self) -> usize {
        self.inner.concurrent_tasks.fetch_add(1, Ordering::Relaxed)
    }

    /// Decrement concurrent task count.
    pub fn complete_task(&self) -> usize {
        self.inner.concurrent_tasks.fetch_sub(1, Ordering::Relaxed).saturating_sub(1)
    }

    /// Get current concurrent task count.
    #[must_use]
    pub fn concurrent_tasks(&self) -> usize {
        self.inner.concurrent_tasks.load(Ordering::Relaxed)
    }

    /// Check if concurrent task limit is exceeded.
    #[must_use]
    pub fn is_concurrency_limit_exceeded(&self) -> bool {
        let current = self.concurrent_tasks();
        current >= self.inner.budget.max_concurrent_subagents as usize
    }

    /// Check if any budget is exceeded.
    #[must_use]
    pub fn check_all_budgets(&self) -> Vec<BudgetExceededInfo> {
        let mut exceeded = Vec::new();

        if self.is_time_budget_exceeded() {
            exceeded.push(BudgetExceededInfo {
                budget_type: BudgetType::Time,
                current: self.elapsed().as_millis() as u64,
                limit: self.inner.budget.max_total_wall_time.map_or(0, |d| d.as_millis() as u64),
            });
        }

        if self.is_token_budget_exceeded() {
            exceeded.push(BudgetExceededInfo {
                budget_type: BudgetType::Tokens,
                current: self.tokens_used(),
                limit: self.inner.budget.token_budget.unwrap_or(0),
            });
        }

        exceeded
    }

    /// Create a TaskFailure for budget exceeded.
    #[must_use]
    pub fn create_budget_exceeded_failure(&self, budget_type: BudgetType) -> TaskFailure {
        let (current, limit) = match budget_type {
            BudgetType::Time => (
                self.elapsed().as_millis() as u64,
                self.inner.budget.max_total_wall_time.map_or(0, |d| d.as_millis() as u64),
            ),
            BudgetType::Tokens => (self.tokens_used(), self.inner.budget.token_budget.unwrap_or(0)),
            BudgetType::Retries => (0, self.inner.budget.max_retries_per_task as u64),
            BudgetType::ConcurrentTasks => {
                (self.concurrent_tasks() as u64, self.inner.budget.max_concurrent_subagents as u64)
            }
            BudgetType::SubagentTasks => (0, self.inner.budget.max_subagent_tasks as u64),
        };

        TaskFailure::budget_exceeded(budget_type, current, limit)
    }

    /// Get budget utilization summary.
    #[must_use]
    pub fn utilization_summary(&self) -> BudgetUtilization {
        let elapsed = self.elapsed();
        let tokens = self.tokens_used();
        let concurrent = self.concurrent_tasks();

        let time_utilization = self
            .inner
            .budget
            .max_total_wall_time
            .map(|max| (elapsed.as_secs_f64() / max.as_secs_f64()).min(1.0));

        let token_utilization =
            self.inner.budget.token_budget.map(|max| tokens as f64 / max as f64);

        let concurrency_utilization =
            concurrent as f64 / self.inner.budget.max_concurrent_subagents as f64;

        BudgetUtilization {
            time: time_utilization,
            tokens: token_utilization,
            concurrency: concurrency_utilization,
        }
    }

    /// Get the budget configuration.
    #[must_use]
    pub fn budget(&self) -> &RunBudget {
        &self.inner.budget
    }

    /// Get the run ID.
    #[must_use]
    pub fn run_id(&self) -> RunId {
        self.inner.run_id
    }

    /// Get the thread ID.
    #[must_use]
    pub fn thread_id(&self) -> ThreadId {
        self.inner.thread_id
    }
}

/// Information about a budget that was exceeded.
#[derive(Debug, Clone)]
pub struct BudgetExceededInfo {
    pub budget_type: BudgetType,
    pub current: u64,
    pub limit: u64,
}

/// Budget utilization percentages (0.0 - 1.0).
#[derive(Debug, Clone)]
pub struct BudgetUtilization {
    pub time: Option<f64>,
    pub tokens: Option<f64>,
    pub concurrency: f64,
}

impl BudgetUtilization {
    /// Check if any budget is above threshold (e.g., 0.8 = 80%).
    #[must_use]
    pub fn is_above_threshold(&self, threshold: f64) -> bool {
        self.time.is_some_and(|t| t > threshold)
            || self.tokens.is_some_and(|tok| tok > threshold)
            || self.concurrency > threshold
    }

    /// Get the highest utilization value.
    #[must_use]
    pub fn max_utilization(&self) -> f64 {
        let mut max = self.concurrency;
        if let Some(time) = self.time {
            max = max.max(time);
        }
        if let Some(tokens) = self.tokens {
            max = max.max(tokens);
        }
        max
    }
}

/// Guard for tracking concurrent task execution.
pub struct ConcurrencyGuard {
    tracker: BudgetTracker,
}

impl ConcurrencyGuard {
    /// Try to acquire a concurrency slot.
    pub fn try_acquire(tracker: &BudgetTracker) -> Option<Self> {
        if tracker.is_concurrency_limit_exceeded() {
            return None;
        }
        tracker.start_task();
        Some(Self { tracker: tracker.clone() })
    }
}

impl Drop for ConcurrencyGuard {
    fn drop(&mut self) {
        self.tracker.complete_task();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_budget() -> RunBudget {
        RunBudget {
            max_total_wall_time: Some(Duration::from_secs(60)),
            token_budget: Some(1000),
            max_retries_per_task: 3,
            max_concurrent_subagents: 4,
            ..Default::default()
        }
    }

    #[test]
    fn test_budget_tracker_creation() {
        let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), create_test_budget());

        assert_eq!(tracker.tokens_used(), 0);
        assert_eq!(tracker.concurrent_tasks(), 0);
        assert!(!tracker.is_time_budget_exceeded());
        assert!(!tracker.is_token_budget_exceeded());
    }

    #[test]
    fn test_token_tracking() {
        let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), create_test_budget());

        tracker.add_tokens(500);
        assert_eq!(tracker.tokens_used(), 500);
        assert!(!tracker.is_token_budget_exceeded());

        tracker.add_tokens(600);
        assert_eq!(tracker.tokens_used(), 1100);
        assert!(tracker.is_token_budget_exceeded());
    }

    #[test]
    fn test_retry_tracking() {
        let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), create_test_budget());

        let task_id = "task-1";
        assert_eq!(tracker.get_retry_count(task_id), 0);
        assert!(!tracker.is_retry_budget_exceeded(task_id));

        tracker.increment_retry(task_id);
        tracker.increment_retry(task_id);
        tracker.increment_retry(task_id);

        assert_eq!(tracker.get_retry_count(task_id), 3);
        assert!(tracker.is_retry_budget_exceeded(task_id));
    }

    #[test]
    fn test_concurrency_tracking() {
        let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), create_test_budget());

        assert_eq!(tracker.concurrent_tasks(), 0);

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
    }

    #[test]
    fn test_utilization_summary() {
        let tracker = BudgetTracker::new(RunId::new_v4(), ThreadId::new_v4(), create_test_budget());

        let summary = tracker.utilization_summary();
        assert!(summary.time.is_some());
        assert!(summary.tokens.is_some());
        assert_eq!(summary.concurrency, 0.0);
    }
}

// ==================== Budget Manager ====================

use crate::budget::BudgetConfig;
use std::collections::HashMap;
use tracing;

/// Manages multiple budget configurations.
pub struct BudgetManager {
    configs: HashMap<String, BudgetConfig>,
    active_config: String,
}

impl BudgetManager {
    /// Create a new budget manager with built-in presets.
    #[must_use]
    pub fn new() -> Self {
        let mut manager = Self { configs: HashMap::new(), active_config: "production".to_string() };

        // Register built-in configurations
        manager.register_builtin("production", BudgetConfig::production_defaults());
        manager.register_builtin("development", BudgetConfig::development_defaults());
        manager.register_builtin("testing", BudgetConfig::testing_defaults());

        manager
    }

    /// Register a built-in configuration.
    pub fn register_builtin(&mut self, name: &str, config: BudgetConfig) {
        self.configs.insert(name.to_string(), config);
    }

    /// Load configuration from a file and register it.
    pub fn load_from_file(
        &mut self,
        name: &str,
        path: &str,
    ) -> Result<(), crate::budget::ConfigError> {
        let config = BudgetConfig::from_file(path)?;

        // Validate configuration
        let warnings = config.validate();
        for warning in &warnings {
            tracing::warn!("Budget config '{}' - {}", name, warning);
        }

        self.configs.insert(name.to_string(), config);
        Ok(())
    }

    /// Set the active configuration by name.
    pub fn set_active(&mut self, name: &str) -> bool {
        if self.configs.contains_key(name) {
            let old_config = self.active_config.clone();
            self.active_config = name.to_string();

            // Log configuration change
            tracing::info!(
                target: "budget_audit",
                old = old_config,
                new = name,
                "Budget configuration changed"
            );

            true
        } else {
            false
        }
    }

    /// Get the active budget configuration.
    #[must_use]
    pub fn get_active_budget(&self) -> Option<RunBudget> {
        self.configs.get(&self.active_config).map(|c| c.to_run_budget())
    }

    /// Get a configuration by name.
    #[must_use]
    pub fn get_config(&self, name: &str) -> Option<&BudgetConfig> {
        self.configs.get(name)
    }

    /// Get the name of the active configuration.
    #[must_use]
    pub fn active_config_name(&self) -> &str {
        &self.active_config
    }

    /// List all registered configuration names.
    #[must_use]
    pub fn list_configs(&self) -> Vec<&String> {
        self.configs.keys().collect()
    }

    /// Create a RunBudget from environment variables.
    #[must_use]
    pub fn from_env() -> RunBudget {
        BudgetConfig::from_env_with_defaults().to_run_budget()
    }
}

impl Default for BudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod budget_manager_tests {
    use super::*;

    #[test]
    fn test_budget_manager_creation() {
        let manager = BudgetManager::new();

        assert!(manager.get_config("production").is_some());
        assert!(manager.get_config("development").is_some());
        assert!(manager.get_config("testing").is_some());
        assert_eq!(manager.active_config_name(), "production");
    }

    #[test]
    fn test_budget_manager_set_active() {
        let mut manager = BudgetManager::new();

        assert!(manager.set_active("development"));
        assert_eq!(manager.active_config_name(), "development");

        assert!(!manager.set_active("nonexistent"));
    }

    #[test]
    fn test_budget_manager_get_budget() {
        let manager = BudgetManager::new();

        let budget = manager.get_active_budget();
        assert!(budget.is_some());

        let budget = budget.expect("Budget should be Some");
        assert_eq!(budget.max_turns, 16); // production default
    }

    #[test]
    fn test_budget_manager_list_configs() {
        let manager = BudgetManager::new();

        let configs = manager.list_configs();
        assert!(configs.contains(&&"production".to_string()));
        assert!(configs.contains(&&"development".to_string()));
        assert!(configs.contains(&&"testing".to_string()));
    }
}
