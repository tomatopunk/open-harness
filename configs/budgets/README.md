# Budget Configuration Guide

This directory contains budget configuration files for the agent-loop-runtime system.

## Overview

The budget system supports multiple configuration methods:
1. **Configuration Files** (YAML/JSON) - Recommended for production
2. **Environment Variables** - Useful for CI/CD and containerized deployments
3. **Builder Pattern** - For programmatic configuration
4. **Configuration Manager** - For runtime configuration management

## Configuration Files

### Location

Configuration files are stored in `configs/budgets/` directory.

### Supported Formats

- **YAML** (`.yaml`, `.yml`) - Recommended for readability
- **JSON** (`.json`) - For programmatic generation

### Example Configurations

#### Production Environment

```bash
# Load production configuration
let config = BudgetConfig::from_file("configs/budgets/production.yaml")?;
let budget = config.to_run_budget();
```

#### Development Environment

```bash
# Load development configuration
let config = BudgetConfig::from_file("configs/budgets/development.yaml")?;
let budget = config.to_run_budget();
```

### Configuration Structure

```yaml
name: "production-v1"           # Configuration name
base_preset: null               # Optional: inherit from preset

# Core parameters
max_turns: 16
max_subagent_tasks: 8
subagent_task_cap_per_response: 4
max_concurrent_subagents: 4
max_concurrent_tool_calls: 8

# Timeouts (human-readable format)
per_subagent_task_timeout: "2m"
max_total_wall_time: "30m"

# Retry configuration
retry:
  max_retries_per_task: 3
  backoff_factor: 2.0
  initial_delay: "1s"
  max_delay: "30s"
  jitter: true

# Token budget (optional)
token_budget: 100000

# Labels for categorization
labels:
  environment: production
  team: ai-platform
```

## Environment Variables

Override configuration using environment variables:

```bash
# Core parameters
export BUDGET_MAX_TURNS=32
export BUDGET_MAX_SUBAGENT_TASKS=16
export BUDGET_MAX_CONCURRENT=8

# Token budget
export BUDGET_TOKEN_LIMIT=200000

# Retry configuration
export BUDGET_MAX_RETRIES=5
```

Load with:

```rust
let budget = RunBudget::from_env_with_defaults();
```

## Configuration Manager

Use `BudgetManager` for runtime configuration management:

```rust
use agent_loop_runtime::BudgetManager;

// Create manager with built-in presets
let mut manager = BudgetManager::new();

// Load custom configuration
manager.load_from_file("custom", "configs/budgets/custom.yaml")?;

// Switch to custom configuration
manager.set_active("custom");

// Get active budget
let budget = manager.get_active_budget().unwrap();

// List all configurations
let configs = manager.list_configs();
```

## Built-in Presets

The system includes three built-in presets:

### Production (`production`)
- Max turns: 16
- Max subagent tasks: 8
- Max concurrent: 4
- Total wall time: 30 minutes
- Token budget: 100k
- Retries: 3

### Development (`development`)
- Max turns: 32
- Max subagent tasks: 16
- Max concurrent: 8
- Total wall time: Unlimited
- Token budget: Unlimited
- Retries: 5

### Testing (`testing`)
- Max turns: 4
- Max subagent tasks: 2
- Max concurrent: 2
- Total wall time: 5 minutes
- Token budget: 10k
- Retries: 1

## Validation

Configurations are automatically validated when loaded. Warnings are logged for:
- Zero values for critical parameters
- Very high concurrency (> 32)
- Very high retry counts (> 10)
- Invalid backoff factors (< 1.0)
- Very long timeouts (> 24 hours)
- Very low token budgets (< 1000)

Strict validation (returns errors):
- Max turns > 100
- Token budget > 1,000,000
- Concurrency > 32
- Backoff factor < 0.1

## Audit Logging

Configuration changes are logged with the `budget_audit` target:

```rust
tracing::info!(
    target: "budget_audit",
    old = "production",
    new = "custom",
    "Budget configuration changed"
);
```

Enable audit logging:

```bash
RUST_LOG=budget_audit=info cargo run
```

## Best Practices

### 1. Use Configuration Files for Production

```yaml
# configs/budgets/production.yaml
name: "production-v1"
# ... configuration ...
```

Version control your configuration files for audit trails.

### 2. Use Environment Variables for Secrets

```bash
export BUDGET_TOKEN_LIMIT=200000  # Can be set by CI/CD
```

### 3. Validate Configurations

```rust
let config = BudgetConfig::from_file("config.yaml")?;

// Check warnings
for warning in config.validate() {
    eprintln!("Warning: {}", warning);
}

// Strict validation
config.validate_strict()?;
```

### 4. Use Labels for Organization

```yaml
labels:
  environment: production
  team: ai-platform
  cost_center: ml-inference
  sla_tier: premium
```

### 5. Monitor Configuration Changes

Enable audit logging in production to track configuration changes.

## Migration Guide

### From Hard-coded Presets

**Before:**
```rust
let budget = RunBudget::production();
```

**After (still works):**
```rust
let budget = RunBudget::production();  // Still supported
```

**Better (config-driven):**
```rust
let config = BudgetConfig::from_file("configs/budgets/production.yaml")?;
let budget = config.to_run_budget();
```

### From Manual Configuration

**Before:**
```rust
let budget = RunBudget {
    max_turns: 16,
    max_subagent_tasks: 8,
    // ... many fields ...
    ..Default::default()
};
```

**After:**
```rust
// Use configuration file
let config = BudgetConfig::from_file("config.yaml")?;
let budget = config.to_run_budget();

// Or use builder pattern
let budget = RunBudget::production()
    .with_max_retries(5)
    .with_token_budget(200_000);
```

## Troubleshooting

### Configuration Not Loading

1. Check file path is correct
2. Verify file format (YAML/JSON)
3. Check for syntax errors in configuration file
4. Review error message for details

### Validation Warnings

1. Review warning message
2. Adjust configuration values
3. Consider if warning is acceptable for your use case

### Configuration Not Taking Effect

1. Verify `set_active()` was called
2. Check configuration name matches
3. Ensure configuration was loaded successfully
4. Review audit logs for configuration changes

## Additional Resources

- [Architecture Documentation](../../docs/subagent-architecture.md)
- [Implementation Summary](../../docs/implementation-summary.md)
- [API Documentation](https://docs.rs/agent-loop-runtime)
