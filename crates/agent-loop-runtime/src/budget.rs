//! Unified budgets for subagents and turns (governance-driven).

use agent_ports::SubtaskPlan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Budget preset templates for common scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPreset {
    /// Development/local testing with relaxed limits
    Development,
    /// Production environment with strict limits
    Production,
    /// Testing/CI environments with minimal resources
    Testing,
    /// High-throughput scenarios
    HighThroughput,
    /// Research and analysis tasks
    Research,
    /// Data validation tasks
    DataValidation,
}

/// Retry configuration for budget execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retries per task
    pub max_retries_per_task: u32,
    /// Exponential backoff factor (e.g., 2.0 means delay doubles each retry)
    pub backoff_factor: f64,
    /// Initial delay for retry backoff
    #[serde(with = "humantime_serde")]
    pub initial_delay: Duration,
    /// Maximum delay for retry backoff
    #[serde(with = "humantime_serde")]
    pub max_delay: Duration,
    /// Whether to add random jitter to delays
    #[serde(default = "default_true")]
    pub jitter: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries_per_task: 3,
            backoff_factor: 2.0,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter: true,
        }
    }
}

/// Serializable budget configuration (supports YAML/JSON/TOML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Configuration name (for logging and monitoring)
    pub name: String,

    /// Base preset to inherit from (optional)
    #[serde(default)]
    pub base_preset: Option<BudgetPreset>,

    /// Core budget parameters
    pub max_turns: u32,
    pub max_subagent_tasks: u32,
    pub subagent_task_cap_per_response: u32,
    pub max_concurrent_subagents: u32,
    pub max_concurrent_tool_calls: u32,

    /// Timeout configurations
    #[serde(with = "humantime_serde", default)]
    pub per_subagent_task_timeout: Option<Duration>,
    #[serde(with = "humantime_serde", default)]
    pub max_total_wall_time: Option<Duration>,

    /// Retry configuration
    #[serde(default)]
    pub retry: RetryConfig,

    /// Token budget (optional)
    pub token_budget: Option<u64>,

    /// Custom labels for categorization
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum ConfigError {
    IoError(String),
    YamlError(String),
    JsonError(String),
    UnsupportedFormat,
    ValidationError(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {e}"),
            Self::YamlError(e) => write!(f, "YAML parsing error: {e}"),
            Self::JsonError(e) => write!(f, "JSON parsing error: {e}"),
            Self::UnsupportedFormat => write!(f, "Unsupported configuration format"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::YamlError(err.to_string())
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

#[derive(Debug, Clone)]
pub enum ConfigWarning {
    ZeroTurns,
    ZeroSubagentTasks,
    ZeroConcurrency,
    HighConcurrency(u32),
    HighRetries(u32),
    InvalidBackoff(f64),
    LongTimeout(u64),
    LowTokenBudget(u64),
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroTurns => write!(f, "max_turns is 0, no turns will be executed"),
            Self::ZeroSubagentTasks => {
                write!(f, "max_subagent_tasks is 0, no subagent tasks will be executed")
            }
            Self::ZeroConcurrency => {
                write!(f, "max_concurrent_subagents is 0, no concurrent execution possible")
            }
            Self::HighConcurrency(val) => {
                write!(f, "max_concurrent_subagents ({val}) is very high")
            }
            Self::HighRetries(val) => {
                write!(f, "max_retries_per_task ({val}) is very high, may cause long delays")
            }
            Self::InvalidBackoff(val) => {
                write!(f, "backoff_factor ({val}) < 1.0, delays will decrease with each retry")
            }
            Self::LongTimeout(secs) => {
                write!(f, "max_total_wall_time > 24 hours ({secs}s), consider reducing")
            }
            Self::LowTokenBudget(val) => {
                write!(f, "token_budget ({val}) is very low, may be exhausted quickly")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConfigValidationError {
    TurnsTooHigh(u32),
    TokenBudgetTooHigh,
    ConcurrencyTooHigh(u32),
    InvalidConfiguration(String),
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TurnsTooHigh(val) => write!(f, "max_turns ({val}) exceeds maximum allowed (100)"),
            Self::TokenBudgetTooHigh => {
                write!(f, "token_budget exceeds maximum allowed (1,000,000)")
            }
            Self::ConcurrencyTooHigh(val) => {
                write!(f, "max_concurrent_subagents ({val}) exceeds maximum allowed (32)")
            }
            Self::InvalidConfiguration(msg) => write!(f, "Invalid configuration: {msg}"),
        }
    }
}

impl std::error::Error for ConfigValidationError {}

#[derive(Debug, Clone, Copy)]
pub struct RunBudget {
    pub max_turns: u32,
    pub max_subagent_tasks: u32,
    /// Hard cap on subtasks taken from a single model plan (per response), before `max_subagent_tasks`.
    pub subagent_task_cap_per_response: u32,
    pub max_concurrent_subagents: u32,
    /// Max parallel tool invocations per model turn (DeerFlow / ToolNode-style).
    pub max_concurrent_tool_calls: u32,
    /// Wall-clock limit for each subagent subtask (`None` = no limit).
    pub per_subagent_task_timeout: Option<Duration>,

    // New fields for enhanced budget control
    /// Maximum retries allowed per individual task
    pub max_retries_per_task: u32,
    /// Exponential backoff factor for retries (e.g., 2.0 means delay doubles each retry)
    pub retry_backoff_factor: f64,
    /// Maximum total wall-clock time for the entire run (`None` = no limit)
    pub max_total_wall_time: Option<Duration>,
    /// Token budget for the run (`None` = no limit)
    pub token_budget: Option<u64>,
    /// Initial delay for retry backoff in milliseconds
    pub retry_initial_delay_ms: u64,
    /// Maximum delay for retry backoff in milliseconds
    pub retry_max_delay_ms: u64,
    /// Whether to add jitter to retry delays
    pub retry_jitter: bool,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_turns: 16,
            max_subagent_tasks: 8,
            subagent_task_cap_per_response: 4,
            max_concurrent_subagents: 4,
            max_concurrent_tool_calls: 8,
            per_subagent_task_timeout: Some(Duration::from_secs(120)),
            // New budget fields with sensible defaults
            max_retries_per_task: 3,
            retry_backoff_factor: 2.0,
            max_total_wall_time: Some(Duration::from_secs(3600)), // 1 hour
            token_budget: None,
            retry_initial_delay_ms: 1000,
            retry_max_delay_ms: 30000, // 30 seconds
            retry_jitter: true,
        }
    }
}

impl BudgetConfig {
    /// Load configuration from a file (YAML or JSON).
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;

        if path.ends_with(".yaml") || path.ends_with(".yml") {
            Ok(serde_yaml::from_str(&content)?)
        } else if path.ends_with(".json") {
            Ok(serde_json::from_str(&content)?)
        } else {
            Err(ConfigError::UnsupportedFormat)
        }
    }

    /// Load configuration from environment variables with production defaults.
    pub fn from_env_with_defaults() -> Self {
        let mut config = Self::production_defaults();

        if let Ok(val) = std::env::var("BUDGET_MAX_TURNS") {
            config.max_turns = val.parse().unwrap_or(config.max_turns);
        }
        if let Ok(val) = std::env::var("BUDGET_MAX_SUBAGENT_TASKS") {
            config.max_subagent_tasks = val.parse().unwrap_or(config.max_subagent_tasks);
        }
        if let Ok(val) = std::env::var("BUDGET_MAX_CONCURRENT") {
            config.max_concurrent_subagents =
                val.parse().unwrap_or(config.max_concurrent_subagents);
        }
        if let Ok(val) = std::env::var("BUDGET_TOKEN_LIMIT") {
            config.token_budget = val.parse().ok();
        }
        if let Ok(val) = std::env::var("BUDGET_MAX_RETRIES") {
            config.retry.max_retries_per_task =
                val.parse().unwrap_or(config.retry.max_retries_per_task);
        }

        config
    }

    /// Convert BudgetConfig to RunBudget.
    pub fn to_run_budget(&self) -> RunBudget {
        RunBudget {
            max_turns: self.max_turns,
            max_subagent_tasks: self.max_subagent_tasks,
            subagent_task_cap_per_response: self.subagent_task_cap_per_response,
            max_concurrent_subagents: self.max_concurrent_subagents,
            max_concurrent_tool_calls: self.max_concurrent_tool_calls,
            per_subagent_task_timeout: self.per_subagent_task_timeout,
            max_total_wall_time: self.max_total_wall_time,
            max_retries_per_task: self.retry.max_retries_per_task,
            retry_backoff_factor: self.retry.backoff_factor,
            token_budget: self.token_budget,
            retry_initial_delay_ms: self.retry.initial_delay.as_millis() as u64,
            retry_max_delay_ms: self.retry.max_delay.as_millis() as u64,
            retry_jitter: self.retry.jitter,
        }
    }

    /// Validate configuration and return warnings.
    pub fn validate(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        if self.max_turns == 0 {
            warnings.push(ConfigWarning::ZeroTurns);
        }
        if self.max_subagent_tasks == 0 {
            warnings.push(ConfigWarning::ZeroSubagentTasks);
        }
        if self.max_concurrent_subagents == 0 {
            warnings.push(ConfigWarning::ZeroConcurrency);
        }
        if self.max_concurrent_subagents > 32 {
            warnings.push(ConfigWarning::HighConcurrency(self.max_concurrent_subagents));
        }
        if self.retry.max_retries_per_task > 10 {
            warnings.push(ConfigWarning::HighRetries(self.retry.max_retries_per_task));
        }
        if self.retry.backoff_factor < 1.0 {
            warnings.push(ConfigWarning::InvalidBackoff(self.retry.backoff_factor));
        }
        if let Some(timeout) = self.max_total_wall_time {
            if timeout.as_secs() > 86400 {
                warnings.push(ConfigWarning::LongTimeout(timeout.as_secs()));
            }
        }
        if let Some(token_budget) = self.token_budget {
            if token_budget < 1000 {
                warnings.push(ConfigWarning::LowTokenBudget(token_budget));
            }
        }

        warnings
    }

    /// Validate configuration strictly (returns errors).
    pub fn validate_strict(&self) -> Result<(), ConfigValidationError> {
        if self.max_turns > 100 {
            return Err(ConfigValidationError::TurnsTooHigh(self.max_turns));
        }
        if self.token_budget.unwrap_or(0) > 1_000_000 {
            return Err(ConfigValidationError::TokenBudgetTooHigh);
        }
        if self.max_concurrent_subagents > 32 {
            return Err(ConfigValidationError::ConcurrencyTooHigh(self.max_concurrent_subagents));
        }
        if self.retry.backoff_factor < 0.1 {
            return Err(ConfigValidationError::InvalidConfiguration(
                "backoff_factor too small (< 0.1)".to_string(),
            ));
        }

        Ok(())
    }

    /// Production environment default configuration.
    pub fn production_defaults() -> Self {
        Self {
            name: "production".to_string(),
            base_preset: None,
            max_turns: 16,
            max_subagent_tasks: 8,
            subagent_task_cap_per_response: 4,
            max_concurrent_subagents: 4,
            max_concurrent_tool_calls: 8,
            per_subagent_task_timeout: Some(Duration::from_secs(120)),
            max_total_wall_time: Some(Duration::from_secs(1800)),
            retry: RetryConfig {
                max_retries_per_task: 3,
                backoff_factor: 2.0,
                initial_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(30),
                jitter: true,
            },
            token_budget: Some(100_000),
            labels: HashMap::new(),
        }
    }

    /// Development environment default configuration.
    pub fn development_defaults() -> Self {
        Self {
            name: "development".to_string(),
            base_preset: None,
            max_turns: 32,
            max_subagent_tasks: 16,
            subagent_task_cap_per_response: 8,
            max_concurrent_subagents: 8,
            max_concurrent_tool_calls: 16,
            per_subagent_task_timeout: Some(Duration::from_secs(300)),
            max_total_wall_time: None,
            retry: RetryConfig {
                max_retries_per_task: 5,
                backoff_factor: 2.0,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(10),
                jitter: true,
            },
            token_budget: None,
            labels: HashMap::new(),
        }
    }

    /// Testing environment default configuration.
    pub fn testing_defaults() -> Self {
        Self {
            name: "testing".to_string(),
            base_preset: None,
            max_turns: 4,
            max_subagent_tasks: 2,
            subagent_task_cap_per_response: 2,
            max_concurrent_subagents: 2,
            max_concurrent_tool_calls: 4,
            per_subagent_task_timeout: Some(Duration::from_secs(30)),
            max_total_wall_time: Some(Duration::from_secs(300)),
            retry: RetryConfig {
                max_retries_per_task: 1,
                backoff_factor: 1.5,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(1),
                jitter: false,
            },
            token_budget: Some(10_000),
            labels: HashMap::new(),
        }
    }

    /// Create from existing RunBudget.
    pub fn from_run_budget(budget: &RunBudget) -> Self {
        Self {
            name: "custom".to_string(),
            base_preset: None,
            max_turns: budget.max_turns,
            max_subagent_tasks: budget.max_subagent_tasks,
            subagent_task_cap_per_response: budget.subagent_task_cap_per_response,
            max_concurrent_subagents: budget.max_concurrent_subagents,
            max_concurrent_tool_calls: budget.max_concurrent_tool_calls,
            per_subagent_task_timeout: budget.per_subagent_task_timeout,
            max_total_wall_time: budget.max_total_wall_time,
            retry: RetryConfig {
                max_retries_per_task: budget.max_retries_per_task,
                backoff_factor: budget.retry_backoff_factor,
                initial_delay: Duration::from_millis(budget.retry_initial_delay_ms),
                max_delay: Duration::from_millis(budget.retry_max_delay_ms),
                jitter: budget.retry_jitter,
            },
            token_budget: budget.token_budget,
            labels: HashMap::new(),
        }
    }
}

/// Truncate a subtask plan to governance + per-response caps (deterministic).
#[must_use]
pub fn truncate_subtask_plan(plan: SubtaskPlan, budget: &RunBudget) -> (SubtaskPlan, bool) {
    let max_t = budget.max_subagent_tasks.max(1) as usize;
    let cap = budget.subagent_task_cap_per_response.max(1) as usize;
    let effective_cap = max_t.min(cap);
    let original_len = plan.tasks.len();
    let truncated_plan =
        SubtaskPlan { tasks: plan.tasks.into_iter().take(effective_cap).collect() };
    let truncated = original_len > truncated_plan.tasks.len();
    (truncated_plan, truncated)
}

impl RunBudget {
    /// Calculate retry delay for a given attempt number (in milliseconds).
    #[must_use]
    pub fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let base_delay = self.retry_initial_delay_ms as f64;
        let backoff = self.retry_backoff_factor;
        let max_delay = self.retry_max_delay_ms as f64;

        // Exponential backoff: base_delay * (backoff ^ (attempt - 1))
        let delay = base_delay * backoff.powi(attempt as i32 - 1);
        let delay = if delay > max_delay { max_delay } else { delay };

        // Add jitter if enabled
        let final_delay = if self.retry_jitter {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            // Add ±20% jitter
            let jitter_range = (delay * 0.2) as u64;
            let jitter = rng.gen_range(0..jitter_range * 2) as f64 - jitter_range as f64;
            (delay + jitter).max(0.0)
        } else {
            delay
        };

        Duration::from_millis(final_delay as u64)
    }

    /// Check if total wall time budget is exceeded.
    #[must_use]
    pub fn is_wall_time_exceeded(&self, elapsed: Duration) -> bool {
        self.max_total_wall_time.is_some_and(|max| elapsed > max)
    }

    /// Check if token budget is exceeded.
    #[must_use]
    pub fn is_token_budget_exceeded(&self, used: u64) -> bool {
        self.token_budget.is_some_and(|max| used > max)
    }

    /// Validate budget configuration for common issues.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.max_turns == 0 {
            warnings.push("max_turns is 0, no turns will be executed".to_string());
        }

        if self.max_subagent_tasks == 0 {
            warnings
                .push("max_subagent_tasks is 0, no subagent tasks will be executed".to_string());
        }

        if self.max_concurrent_subagents == 0 {
            warnings.push(
                "max_concurrent_subagents is 0, no concurrent execution possible".to_string(),
            );
        }

        if self.max_retries_per_task > 10 {
            warnings.push(format!(
                "max_retries_per_task ({}) is very high, may cause long delays",
                self.max_retries_per_task
            ));
        }

        if self.retry_backoff_factor < 1.0 {
            warnings.push(
                "retry_backoff_factor < 1.0, delays will decrease with each retry".to_string(),
            );
        }

        if let Some(timeout) = self.max_total_wall_time {
            if timeout.as_secs() > 86400 {
                warnings.push("max_total_wall_time > 24 hours, consider reducing".to_string());
            }
        }

        if let Some(token_budget) = self.token_budget {
            if token_budget < 1000 {
                warnings.push(
                    "token_budget is very low (< 1000), may be exhausted quickly".to_string(),
                );
            }
        }

        warnings
    }

    /// Create a budget configuration for development/local testing.
    #[must_use]
    pub fn development() -> Self {
        Self {
            max_turns: 32,
            max_subagent_tasks: 16,
            subagent_task_cap_per_response: 8,
            max_concurrent_subagents: 8,
            max_concurrent_tool_calls: 16,
            per_subagent_task_timeout: Some(Duration::from_secs(300)), // 5 minutes
            max_retries_per_task: 5,
            retry_backoff_factor: 2.0,
            max_total_wall_time: None, // No time limit for development
            token_budget: None,        // No token limit for development
            retry_initial_delay_ms: 500,
            retry_max_delay_ms: 10000,
            retry_jitter: true,
        }
    }

    /// Create a budget configuration for production use.
    #[must_use]
    pub fn production() -> Self {
        Self {
            max_turns: 16,
            max_subagent_tasks: 8,
            subagent_task_cap_per_response: 4,
            max_concurrent_subagents: 4,
            max_concurrent_tool_calls: 8,
            per_subagent_task_timeout: Some(Duration::from_secs(120)), // 2 minutes
            max_retries_per_task: 3,
            retry_backoff_factor: 2.0,
            max_total_wall_time: Some(Duration::from_secs(1800)), // 30 minutes
            token_budget: Some(100_000),                          // 100k tokens
            retry_initial_delay_ms: 1000,
            retry_max_delay_ms: 30000,
            retry_jitter: true,
        }
    }

    /// Create a budget configuration for testing/CI environments.
    #[must_use]
    pub fn testing() -> Self {
        Self {
            max_turns: 4,
            max_subagent_tasks: 2,
            subagent_task_cap_per_response: 2,
            max_concurrent_subagents: 2,
            max_concurrent_tool_calls: 4,
            per_subagent_task_timeout: Some(Duration::from_secs(30)), // 30 seconds
            max_retries_per_task: 1,
            retry_backoff_factor: 1.5,
            max_total_wall_time: Some(Duration::from_secs(300)), // 5 minutes
            token_budget: Some(10_000),                          // 10k tokens
            retry_initial_delay_ms: 100,
            retry_max_delay_ms: 1000,
            retry_jitter: false,
        }
    }

    /// Create a budget configuration for high-throughput scenarios.
    #[must_use]
    pub fn high_throughput() -> Self {
        Self {
            max_turns: 8,
            max_subagent_tasks: 32,
            subagent_task_cap_per_response: 16,
            max_concurrent_subagents: 16,
            max_concurrent_tool_calls: 32,
            per_subagent_task_timeout: Some(Duration::from_secs(60)), // 1 minute
            max_retries_per_task: 2,
            retry_backoff_factor: 1.5,
            max_total_wall_time: Some(Duration::from_secs(600)), // 10 minutes
            token_budget: Some(500_000),                         // 500k tokens
            retry_initial_delay_ms: 500,
            retry_max_delay_ms: 5000,
            retry_jitter: true,
        }
    }

    /// Create a budget configuration for research/analysis tasks.
    #[must_use]
    pub fn research_task() -> Self {
        Self {
            max_turns: 24,
            max_subagent_tasks: 12,
            subagent_task_cap_per_response: 6,
            max_concurrent_subagents: 6,
            max_concurrent_tool_calls: 12,
            per_subagent_task_timeout: Some(Duration::from_secs(180)), // 3 minutes
            max_retries_per_task: 4,
            retry_backoff_factor: 2.0,
            max_total_wall_time: Some(Duration::from_secs(3600)), // 1 hour
            token_budget: Some(200_000),                          // 200k tokens
            retry_initial_delay_ms: 1000,
            retry_max_delay_ms: 20000,
            retry_jitter: true,
        }
    }

    /// Create a budget configuration for data validation tasks.
    #[must_use]
    pub fn data_validation() -> Self {
        Self {
            max_turns: 12,
            max_subagent_tasks: 6,
            subagent_task_cap_per_response: 3,
            max_concurrent_subagents: 3,
            max_concurrent_tool_calls: 6,
            per_subagent_task_timeout: Some(Duration::from_secs(90)), // 90 seconds
            max_retries_per_task: 2,
            retry_backoff_factor: 2.0,
            max_total_wall_time: Some(Duration::from_secs(900)), // 15 minutes
            token_budget: Some(50_000),                          // 50k tokens
            retry_initial_delay_ms: 800,
            retry_max_delay_ms: 15000,
            retry_jitter: true,
        }
    }

    /// Apply conservative limits to a budget (reduce by 50%).
    #[must_use]
    pub fn conservative(&self) -> Self {
        let mut budget = *self;
        budget.max_turns = (budget.max_turns / 2).max(1);
        budget.max_subagent_tasks = (budget.max_subagent_tasks / 2).max(1);
        budget.max_concurrent_subagents = (budget.max_concurrent_subagents / 2).max(1);
        budget.max_concurrent_tool_calls = (budget.max_concurrent_tool_calls / 2).max(1);
        if let Some(tokens) = budget.token_budget {
            budget.token_budget = Some(tokens / 2);
        }
        budget
    }

    /// Apply generous limits to a budget (increase by 50%).
    #[must_use]
    pub fn generous(&self) -> Self {
        let mut budget = *self;
        budget.max_turns = (budget.max_turns * 3 / 2).max(1);
        budget.max_subagent_tasks = (budget.max_subagent_tasks * 3 / 2).max(1);
        budget.max_concurrent_subagents = (budget.max_concurrent_subagents * 3 / 2).max(1);
        budget.max_concurrent_tool_calls = (budget.max_concurrent_tool_calls * 3 / 2).max(1);
        if let Some(tokens) = budget.token_budget {
            budget.token_budget = Some(tokens * 3 / 2);
        }
        budget
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn test_budget_presets() {
        let dev = RunBudget::development();
        assert!(dev.max_total_wall_time.is_none());
        assert!(dev.token_budget.is_none());
        assert_eq!(dev.max_retries_per_task, 5);

        let prod = RunBudget::production();
        assert!(prod.max_total_wall_time.is_some());
        assert!(prod.token_budget.is_some());
        assert_eq!(prod.max_retries_per_task, 3);

        let test = RunBudget::testing();
        assert_eq!(test.max_turns, 4);
        assert_eq!(test.max_retries_per_task, 1);
    }

    #[test]
    fn test_budget_validation() {
        let budget = RunBudget::default();
        let warnings = budget.validate();
        assert!(warnings.is_empty());

        let invalid_budget = RunBudget {
            max_turns: 0,
            max_subagent_tasks: 0,
            max_concurrent_subagents: 0,
            ..Default::default()
        };
        let warnings = invalid_budget.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.len() >= 3);
    }

    #[test]
    fn test_conservative_and_generous() {
        let base = RunBudget::production();

        let conservative = base.conservative();
        assert!(conservative.max_subagent_tasks < base.max_subagent_tasks);

        let generous = base.generous();
        assert!(generous.max_subagent_tasks > base.max_subagent_tasks);
    }

    #[test]
    fn test_specialized_budgets() {
        let research = RunBudget::research_task();
        assert_eq!(research.max_subagent_tasks, 12);
        assert!(research.token_budget.is_some());

        let validation = RunBudget::data_validation();
        assert_eq!(validation.max_subagent_tasks, 6);

        let throughput = RunBudget::high_throughput();
        assert_eq!(throughput.max_concurrent_subagents, 16);
    }
}
