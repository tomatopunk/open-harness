//! Subagent Workflow Example
//!
//! This example demonstrates how to use the subagent architecture for complex tasks:
//! - Task decomposition using LLM or templates
//! - Concurrent execution with budget control
//! - Intelligent retry and fallback strategies
//! - Result merging with different strategies
//!
//! Run with: `cargo run --example subagent_workflow`

#![allow(dead_code)]

use agent_adapters::EnhancedSubagentAdapter;
use agent_loop_runtime::RunBudget;
use agent_ports::{
    error_helpers, merger_presets, DecompositionStrategy, FallbackStrategy, RetryPolicy,
    TaskTemplate,
};

/// Example 1: LLM-based task decomposition with retry and fallback
async fn example_llm_decomposition() {
    println!("\n=== Example 1: LLM-based Task Decomposition ===\n");

    // Configure budget for production use
    let budget = RunBudget::production();
    println!("Budget configured for production:");
    println!("  - Max subagent tasks: {}", budget.max_subagent_tasks);
    println!("  - Max concurrent: {}", budget.max_concurrent_subagents);
    println!("  - Token budget: {:?}", budget.token_budget);

    // Configure retry policy
    let retry_policy = RetryPolicy::default()
        .with_max_retries(3)
        .with_initial_delay(1000)
        .with_max_delay(30000)
        .with_jitter(true);

    // Configure fallback strategy
    let fallback_strategy = FallbackStrategy::UseSimplified {
        task_goal: "Simplified version of the task".to_string(),
        task_input: serde_json::json!(null),
    };

    // Create enhanced subagent adapter
    let _adapter = EnhancedSubagentAdapter::with_params_and_config(
        agent_ports::SubagentExecuteParams::default(),
        agent_adapters::subagent_enhanced::EnhancedExecutionConfig::with_retry()
            .with_retry_policy(retry_policy.clone())
            .with_fallback_strategy(fallback_strategy),
    );

    println!("\nEnhanced adapter created with:");
    println!("  - Retry enabled: true");
    println!("  - Max retries: {}", retry_policy.max_retries);
    println!("  - Fallback: UseSimplified");

    // In a real implementation, you would:
    // 1. Create an LLMPort (e.g., OpenAI adapter)
    // 2. Create LlmTaskDecomposer with the port
    // 3. Decompose a goal into subtasks
    // 4. Execute the plan with the adapter
    // 5. Merge results

    println!("\nNote: This example shows the configuration setup.");
    println!("Actual LLM calls require a configured LLMPort implementation.");
}

/// Example 2: Template-based decomposition with consensus merging
async fn example_template_consensus() {
    println!("\n=== Example 2: Template-based with Consensus ===\n");

    // Create a data validation template
    let template = TaskTemplate::data_validation_template();
    println!("Created data validation template:");
    println!("  - ID: {}", template.id);
    println!("  - Subtasks: {}", template.subtasks.len());
    for (i, task) in template.subtasks.iter().enumerate() {
        println!("    {}. {} (budget: {} steps)", i + 1, task.goal, task.budget_steps);
    }

    // Create decomposition strategy
    let _strategy = DecompositionStrategy::hybrid(template.clone());

    // Create consensus merger configuration
    let _consensus_config = agent_ports::ConsensusConfig::supermajority().with_vote_field("answer");

    println!("\nConsensus merger configured:");
    println!("  - Method: Supermajority (67% threshold)");
    println!("  - Vote field: answer");

    // In a real implementation:
    // 1. Decompose using the hybrid strategy
    // 2. Execute tasks in parallel
    // 3. Merge results using consensus

    println!("\nThis setup is ideal for data validation tasks where");
    println!("multiple sources need to agree on the result.");
}

/// Example 3: Research task with LLM summary merging
async fn example_research_summary() {
    println!("\n=== Example 3: Research Task with LLM Summary ===\n");

    // Use research-optimized budget
    let budget = RunBudget::research_task();
    println!("Research task budget:");
    println!("  - Max turns: {}", budget.max_turns);
    println!("  - Max subagent tasks: {}", budget.max_subagent_tasks);
    println!("  - Max concurrent: {}", budget.max_concurrent_subagents);
    println!("  - Per-task timeout: {:?}", budget.per_subagent_task_timeout);

    // Create research hybrid strategy
    let _strategy = DecompositionStrategy::research_hybrid();

    // LLM summary would be created with:
    // let merger = merger_presets::llm_detailed_summary(llm_port);

    println!("\nResearch strategy configured with:");
    println!("  - Template: Research");
    println!("  - LLM extension: Enabled");
    println!("  - Merge: LLM detailed summary");
}

/// Example 4: Error handling and classification
fn example_error_handling() {
    println!("\n=== Example 4: Error Handling ===\n");

    // Test error classification
    let errors = vec![
        "Connection timeout",
        "Rate limit exceeded",
        "Resource not found",
        "Invalid parameter",
        "Network unavailable",
    ];

    println!("Error classification examples:\n");
    for error in errors {
        let failure = error_helpers::classify_error_msg(error);
        let should_retry = failure.should_retry();

        println!("Error: '{}'", error);
        println!("  - Type: {:?}", std::mem::discriminant(&failure));
        println!("  - Should retry: {}", should_retry);
        println!();
    }

    // Demonstrate retry delay calculation
    let policy =
        RetryPolicy::default().with_initial_delay(1000).with_backoff_factor(2.0).with_jitter(false);

    println!("Retry delay schedule (exponential backoff):");
    for attempt in 1..=5 {
        let delay = policy.calculate_delay(attempt);
        println!("  Attempt {}: {}ms", attempt, delay);
    }
}

/// Example 5: Budget presets and validation
fn example_budget_presets() {
    println!("\n=== Example 5: Budget Presets ===\n");

    let budgets = vec![
        ("Development", RunBudget::development()),
        ("Production", RunBudget::production()),
        ("Testing", RunBudget::testing()),
        ("High Throughput", RunBudget::high_throughput()),
        ("Research", RunBudget::research_task()),
        ("Data Validation", RunBudget::data_validation()),
    ];

    println!("Budget presets comparison:\n");
    println!(
        "{:<20} {:>8} {:>10} {:>12} {:>15}",
        "Preset", "Turns", "Tasks", "Concurrent", "Token Budget"
    );
    println!("{:-<70}", "");

    for (name, budget) in &budgets {
        let tokens = budget
            .token_budget
            .map(|t| if t >= 1000 { format!("{}k", t / 1000) } else { t.to_string() })
            .unwrap_or("∞".to_string());

        println!(
            "{:<20} {:>8} {:>10} {:>12} {:>15}",
            name,
            budget.max_turns,
            budget.max_subagent_tasks,
            budget.max_concurrent_subagents,
            tokens
        );
    }

    // Validate a budget
    println!("\nBudget validation:");
    let invalid_budget = RunBudget {
        max_turns: 0,
        max_subagent_tasks: 0,
        max_concurrent_subagents: 0,
        ..Default::default()
    };

    let warnings = invalid_budget.validate();
    if !warnings.is_empty() {
        println!("  Warnings for invalid budget:");
        for warning in warnings {
            println!("    - {}", warning);
        }
    }
}

/// Example 6: Merger presets
fn example_merger_presets() {
    println!("\n=== Example 6: Merger Presets ===\n");

    // Concatenation merger
    let _concat = merger_presets::concatenate_with_separator("\n\n---\n\n");
    println!("Concatenate merger created with custom separator");

    // Consensus mergers
    let _majority = merger_presets::consensus_majority();
    let _supermajority = merger_presets::consensus_supermajority();
    let _approval = merger_presets::consensus_approval();

    println!("\nConsensus merger presets:");
    println!("  - Simple majority (50% threshold)");
    println!("  - Supermajority (67% threshold)");
    println!("  - Approval voting");

    // Note: LLM mergers require an LLMPort instance
    println!("\nLLM summary mergers available:");
    println!("  - llm_summary (default config)");
    println!("  - llm_detailed_summary (1000 tokens)");
    println!("  - llm_executive_summary (300 tokens)");
}

/// Example 7: Complete workflow (pseudo-code)
fn example_complete_workflow() {
    println!("\n=== Example 7: Complete Workflow (Pseudo-code) ===\n");

    println!("Complete subagent workflow structure:\n");
    println!("```rust");
    println!("async fn run_subagent_workflow(goal: &str) -> Result<(), Box<dyn Error>> {{");
    println!("    // 1. Setup");
    println!("    let budget = RunBudget::production();");
    println!("    let llm_port = Arc::new(OpenAiAdapter::new(\"gpt-4\"));");
    println!("    ");
    println!("    // 2. Decompose goal");
    println!("    let decomposer = LlmTaskDecomposer::with_default_config(llm_port.clone());");
    println!("    let context = TaskContext::new(/* ... */);");
    println!("    let plan = decomposer.decompose(goal, &context).await?;");
    println!("    ");
    println!("    // 3. Execute with retry/fallback");
    println!("    let adapter = EnhancedSubagentAdapter::with_config(");
    println!("        EnhancedExecutionConfig::with_retry()");
    println!("            .with_retry_policy(RetryPolicy::default())");
    println!("            .with_fallback_strategy(FallbackStrategy::SkipAndContinue)");
    println!("    );");
    println!("    let results = adapter.execute_plan(");
    println!("        run_id, thread_id, &plan, &state, &params, &mut sink");
    println!("    ).await?;");
    println!("    ");
    println!("    // 4. Merge results");
    println!("    let merger = merger_presets::llm_summary(llm_port);");
    println!("    let merged = merger.merge(&context, &results).await?;");
    println!("    ");
    println!("    Ok(())");
    println!("}}");
    println!("```");
}

#[tokio::main]
async fn main() {
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║     Subagent Architecture Workflow Examples              ║");
    println!("╚═══════════════════════════════════════════════════════════╝");

    // Run all examples
    example_llm_decomposition().await;
    example_template_consensus().await;
    example_research_summary().await;
    example_error_handling();
    example_budget_presets();
    example_merger_presets();
    example_complete_workflow();

    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║                  Examples Complete                       ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");
}
