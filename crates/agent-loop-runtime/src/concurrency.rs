//! Concurrency control for subagent execution with budget enforcement.

use crate::budget::RunBudget;
use agent_ports::{AgentEvent, BudgetType, EventSink, RunId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};

/// Tracks concurrent execution metrics.
pub struct AtomicMetrics {
    concurrent_tasks: AtomicU64,
    total_tasks_started: AtomicU64,
    total_tasks_completed: AtomicU64,
    peak_concurrency: AtomicU64,
}

impl AtomicMetrics {
    pub fn new() -> Self {
        Self {
            concurrent_tasks: AtomicU64::new(0),
            total_tasks_started: AtomicU64::new(0),
            total_tasks_completed: AtomicU64::new(0),
            peak_concurrency: AtomicU64::new(0),
        }
    }

    pub fn increment_concurrent_tasks(&self) {
        let current = self.concurrent_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        self.total_tasks_started.fetch_add(1, Ordering::SeqCst);

        // Update peak concurrency
        let mut peak = self.peak_concurrency.load(Ordering::SeqCst);
        while current > peak {
            match self.peak_concurrency.compare_exchange_weak(
                peak,
                current,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    pub fn decrement_concurrent_tasks(&self) {
        self.concurrent_tasks.fetch_sub(1, Ordering::SeqCst);
        self.total_tasks_completed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_concurrent_tasks(&self) -> u64 {
        self.concurrent_tasks.load(Ordering::SeqCst)
    }

    pub fn get_peak_concurrency(&self) -> u64 {
        self.peak_concurrency.load(Ordering::SeqCst)
    }

    pub fn get_total_tasks_started(&self) -> u64 {
        self.total_tasks_started.load(Ordering::SeqCst)
    }

    pub fn get_total_tasks_completed(&self) -> u64 {
        self.total_tasks_completed.load(Ordering::SeqCst)
    }
}

impl Default for AtomicMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for a concurrency permit.
pub struct Permit<'a> {
    _permit: SemaphorePermit<'a>,
    metrics: Arc<AtomicMetrics>,
}

impl<'a> Drop for Permit<'a> {
    fn drop(&mut self) {
        self.metrics.decrement_concurrent_tasks();
    }
}

/// Controls concurrent execution with budget enforcement.
pub struct ConcurrencyController {
    semaphore: Arc<Semaphore>,
    budget: RunBudget,
    metrics: Arc<AtomicMetrics>,
    start_time: Instant,
}

impl ConcurrencyController {
    /// Create a new concurrency controller.
    #[must_use]
    pub fn new(budget: RunBudget) -> Self {
        let max_concurrent = budget.max_concurrent_subagents.max(1) as usize;
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            budget,
            metrics: Arc::new(AtomicMetrics::new()),
            start_time: Instant::now(),
        }
    }

    /// Acquire a permit for concurrent execution.
    pub async fn acquire(
        &self,
        run_id: RunId,
        sink: &mut EventSink,
    ) -> Result<Permit<'_>, BudgetExceeded> {
        // Check wall time budget
        let elapsed = self.start_time.elapsed();
        if self.budget.is_wall_time_exceeded(elapsed) {
            let limit = self.budget.max_total_wall_time.unwrap_or_default();
            sink.push(AgentEvent::BudgetExceeded {
                run_id,
                budget_type: BudgetType::Time,
                current: elapsed.as_millis() as u64,
                limit: limit.as_millis() as u64,
            });
            return Err(BudgetExceeded::Time(elapsed));
        }

        // Try to acquire permit
        let permit = self.semaphore.acquire().await.map_err(|_| BudgetExceeded::ConcurrentTasks)?;

        // Update metrics
        self.metrics.increment_concurrent_tasks();

        Ok(Permit { _permit: permit, metrics: Arc::clone(&self.metrics) })
    }

    /// Get the metrics tracker.
    #[must_use]
    pub fn metrics(&self) -> Arc<AtomicMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Get elapsed time since controller creation.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Check if wall time budget is exceeded.
    #[must_use]
    pub fn is_wall_time_exceeded(&self) -> bool {
        self.budget.is_wall_time_exceeded(self.start_time.elapsed())
    }
}

/// Budget exceeded errors.
#[derive(Debug, thiserror::Error)]
pub enum BudgetExceeded {
    #[error("wall time budget exceeded: {0:?}")]
    Time(Duration),
    #[error("concurrent tasks budget exceeded")]
    ConcurrentTasks,
    #[error("token budget exceeded")]
    Tokens,
    #[error("retry budget exceeded")]
    Retries,
    #[error("subagent tasks budget exceeded")]
    SubagentTasks,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_tracking() {
        let metrics = Arc::new(AtomicMetrics::new());

        metrics.increment_concurrent_tasks();
        assert_eq!(metrics.get_concurrent_tasks(), 1);
        assert_eq!(metrics.get_total_tasks_started(), 1);

        metrics.increment_concurrent_tasks();
        assert_eq!(metrics.get_concurrent_tasks(), 2);
        assert_eq!(metrics.get_peak_concurrency(), 2);

        metrics.decrement_concurrent_tasks();
        assert_eq!(metrics.get_concurrent_tasks(), 1);
        assert_eq!(metrics.get_total_tasks_completed(), 1);
    }

    #[test]
    fn test_budget_default() {
        let budget = RunBudget::default();
        let controller = ConcurrencyController::new(budget);

        assert!(!controller.is_wall_time_exceeded());
        assert_eq!(controller.metrics().get_concurrent_tasks(), 0);
    }
}
