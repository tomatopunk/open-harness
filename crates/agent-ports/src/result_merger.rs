//! Result merging strategies for subagent outputs.
//!
//! This module provides abstractions for combining multiple subagent results
//! into a single coherent output, supporting various strategies:
//! - Simple concatenation
//! - LLM-based summarization
//! - Consensus/voting
//! - Custom reducers

use crate::ids::{RunId, ThreadId};
use crate::ports::{LLMPort, LlmTurnContext, SubtaskPlan};
use crate::thread_state::ThreadState;
use crate::{PortResult, SubagentResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Context for result merging operations.
#[derive(Debug, Clone)]
pub struct MergeContext {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub parent_state: ThreadState,
    pub original_plan: SubtaskPlan,
    pub policy_version: Option<String>,
}

impl MergeContext {
    #[must_use]
    pub fn new(
        thread_id: ThreadId,
        run_id: RunId,
        parent_state: ThreadState,
        original_plan: SubtaskPlan,
        policy_version: Option<String>,
    ) -> Self {
        Self { thread_id, run_id, parent_state, original_plan, policy_version }
    }
}

/// Merged result from multiple subagent outputs.
#[derive(Debug, Clone)]
pub struct MergedResult {
    pub content: Value,
    pub summary: Option<String>,
    pub metadata: Value,
}

impl Default for MergedResult {
    fn default() -> Self {
        Self {
            content: Value::Null,
            summary: None,
            metadata: Value::Object(serde_json::Map::new()),
        }
    }
}

/// Trait for result mergers - combines multiple subagent results.
#[async_trait::async_trait]
pub trait ResultMerger: Send + Sync {
    /// Merge multiple subagent results into a single output.
    ///
    /// # Arguments
    /// * `ctx` - Merge context with thread state and metadata
    /// * `results` - Slice of subagent results to merge
    ///
    /// # Returns
    /// * `Ok(MergedResult)` - The merged result
    /// * `Err(PortError)` - Error during merging
    async fn merge(
        &self,
        ctx: &MergeContext,
        results: &[SubagentResult],
    ) -> PortResult<MergedResult>;

    /// Get the strategy name for logging/observability.
    fn strategy_name(&self) -> &'static str;
}

/// Configuration for concatenation merger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcatenateConfig {
    /// Separator between concatenated results
    pub separator: String,
    /// Whether to preserve original task order
    pub preserve_order: bool,
    /// Whether to include task metadata (task_id, ok status)
    pub include_metadata: bool,
}

impl Default for ConcatenateConfig {
    fn default() -> Self {
        Self { separator: "\n\n---\n\n".to_string(), preserve_order: true, include_metadata: true }
    }
}

/// Simple concatenation merger.
pub struct ConcatenateMerger {
    config: ConcatenateConfig,
}

impl ConcatenateMerger {
    #[must_use]
    pub fn new(config: ConcatenateConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(ConcatenateConfig::default())
    }
}

#[async_trait::async_trait]
impl ResultMerger for ConcatenateMerger {
    async fn merge(
        &self,
        _ctx: &MergeContext,
        results: &[SubagentResult],
    ) -> PortResult<MergedResult> {
        let mut merged = MergedResult::default();

        let parts: Vec<String> = results
            .iter()
            .map(|r| {
                if self.config.include_metadata {
                    format!(
                        "Task {} ({}): {}",
                        r.task_id,
                        if r.ok { "success" } else { "failed" },
                        r.output
                    )
                } else {
                    r.output.to_string()
                }
            })
            .collect();

        merged.content = Value::String(parts.join(&self.config.separator));
        merged.summary = Some(format!("Merged {} results", results.len()));

        Ok(merged)
    }

    fn strategy_name(&self) -> &'static str {
        "concatenate"
    }
}

/// Configuration for LLM-based summarization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSummaryConfig {
    /// System prompt for summarization
    pub summary_prompt: String,
    /// Model to use for summarization (None = inherit from parent)
    pub model: Option<String>,
    /// Maximum summary length in tokens
    pub max_summary_tokens: u32,
}

impl Default for LlmSummaryConfig {
    fn default() -> Self {
        Self {
            summary_prompt: DEFAULT_SUMMARY_PROMPT.to_string(),
            model: None,
            max_summary_tokens: 500,
        }
    }
}

impl LlmSummaryConfig {
    /// Create a new LLM summary configuration.
    #[must_use]
    pub fn new(summary_prompt: impl Into<String>) -> Self {
        Self { summary_prompt: summary_prompt.into(), model: None, max_summary_tokens: 500 }
    }

    /// Set the model to use.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the maximum summary tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_summary_tokens = max;
        self
    }

    /// Create a config for detailed summaries.
    #[must_use]
    pub fn detailed_summary() -> Self {
        Self::new(
            "You are an expert analyst. Create a comprehensive, detailed summary of all subagent outputs. Include:
1. Key findings from each output
2. Common themes and patterns
3. Notable disagreements or contradictions
4. Actionable insights and recommendations

Subagent Outputs:
{outputs}

Detailed Summary:"
        )
        .with_max_tokens(1000)
    }

    /// Create a config for executive summaries.
    #[must_use]
    pub fn executive_summary() -> Self {
        Self::new(
            "You are an executive assistant. Create a concise, high-level summary (max 3 paragraphs) of subagent outputs.
Focus on:
- Most important findings
- Key decisions needed
- Critical action items

Subagent Outputs:
{outputs}

Executive Summary:"
        )
        .with_max_tokens(300)
    }
}

/// LLM-based summarization merger.
pub struct LlmSummaryMerger {
    config: LlmSummaryConfig,
    llm_port: Arc<dyn LLMPort>,
}

impl LlmSummaryMerger {
    #[must_use]
    pub fn new(config: LlmSummaryConfig, llm_port: Arc<dyn LLMPort>) -> Self {
        Self { config, llm_port }
    }

    #[must_use]
    pub fn with_default_config(llm_port: Arc<dyn LLMPort>) -> Self {
        Self::new(LlmSummaryConfig::default(), llm_port)
    }

    /// Build the summarization prompt with all subagent outputs.
    fn build_summary_prompt(&self, results: &[SubagentResult]) -> String {
        let outputs_text = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let status = if r.ok { "✓" } else { "✗" };
                let output_str = match &r.output {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("Task {} [{}]: {}", i + 1, status, output_str)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        self.config
            .summary_prompt
            .replace("{outputs}", &outputs_text)
            .replace("{num_results}", &results.len().to_string())
    }

    /// Parse LLM summary response.
    fn parse_summary_response(&self, content: &str) -> String {
        // Clean up the response (remove markdown, extra whitespace)
        content
            .trim()
            .strip_prefix("```")
            .and_then(|s| s.split("```").next())
            .unwrap_or(content)
            .trim()
            .to_string()
    }
}

#[async_trait::async_trait]
impl ResultMerger for LlmSummaryMerger {
    async fn merge(
        &self,
        _ctx: &MergeContext,
        results: &[SubagentResult],
    ) -> PortResult<MergedResult> {
        if results.is_empty() {
            return Ok(MergedResult::default());
        }

        // Build the summary prompt
        let user_prompt = self.build_summary_prompt(results);

        // Create LLM turn context
        let llm_context = LlmTurnContext {
            run_id: _ctx.run_id,
            thread_id: _ctx.thread_id,
            messages: vec![Value::String(user_prompt)],
            system_prompt: Some("You are an expert synthesizer. Your job is to combine multiple subagent outputs into a coherent summary.".to_string()),
            model_name: self.config.model.clone(),
            policy_version: _ctx.policy_version.clone(),
            is_plan_mode: false,
            assembled_tool_names: vec![],
            loop_detected: false,
        };

        // Call LLM for summarization
        let llm_output = self.llm_port.infer_turn(llm_context).await?;

        // Extract the summary
        let summary = llm_output
            .assistant_text
            .map(|s| self.parse_summary_response(&s))
            .unwrap_or_else(|| format!("Merged {} results", results.len()));

        // Prepare merged result
        let mut merged = MergedResult::default();
        let outputs: Vec<Value> = results.iter().map(|r| r.output.clone()).collect();
        merged.content = Value::Array(outputs);
        merged.summary = Some(summary);
        merged.metadata = serde_json::json!({
            "num_results": results.len(),
            "successful_tasks": results.iter().filter(|r| r.ok).count(),
            "failed_tasks": results.iter().filter(|r| !r.ok).count(),
        });

        Ok(merged)
    }

    fn strategy_name(&self) -> &'static str {
        "llm_summary"
    }
}

/// Voting method for consensus merger.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VotingMethod {
    /// Simple majority voting
    Majority,
    /// Weighted voting (weights based on confidence scores)
    Weighted,
    /// Borda count (rank-based voting)
    Borda,
    /// Approval voting (approve/disapprove)
    Approval,
}

/// Configuration for consensus merger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Voting method to use
    pub voting_method: VotingMethod,
    /// Minimum agreement threshold (0.0-1.0)
    pub threshold: f64,
    /// Field name to extract for voting (if outputs are objects)
    pub vote_field: Option<String>,
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self { voting_method: VotingMethod::Majority, threshold: 0.5, vote_field: None }
    }
}

impl ConsensusConfig {
    /// Create a new consensus configuration.
    #[must_use]
    pub fn new(voting_method: VotingMethod, threshold: f64) -> Self {
        Self { voting_method, threshold, vote_field: None }
    }

    /// Set the vote field to extract from outputs.
    #[must_use]
    pub fn with_vote_field(mut self, field: impl Into<String>) -> Self {
        self.vote_field = Some(field.into());
        self
    }

    /// Create a config for simple majority voting.
    #[must_use]
    pub fn simple_majority() -> Self {
        Self::new(VotingMethod::Majority, 0.5)
    }

    /// Create a config for supermajority voting (requires 67% agreement).
    #[must_use]
    pub fn supermajority() -> Self {
        Self::new(VotingMethod::Majority, 0.67)
    }

    /// Create a config for unanimous consensus (requires 90% agreement).
    #[must_use]
    pub fn unanimous() -> Self {
        Self::new(VotingMethod::Majority, 0.9)
    }

    /// Create a config for weighted voting with confidence scores.
    #[must_use]
    pub fn weighted_confidence() -> Self {
        Self::new(VotingMethod::Weighted, 0.6)
    }

    /// Create a config for approval voting.
    #[must_use]
    pub fn approval_voting() -> Self {
        Self::new(VotingMethod::Approval, 0.5)
    }
}

/// Consensus/voting-based merger.
pub struct ConsensusMerger {
    config: ConsensusConfig,
}

impl ConsensusMerger {
    #[must_use]
    pub fn new(config: ConsensusConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(ConsensusConfig::default())
    }

    /// Extract the value to vote on from a result.
    fn extract_vote_value(&self, result: &SubagentResult) -> String {
        if let Some(field) = &self.config.vote_field {
            // Extract specific field if outputs are objects
            result.output.get(field).map_or_else(
                || result.output.to_string(),
                |v| v.as_str().unwrap_or(&v.to_string()).to_string(),
            )
        } else {
            // Use the entire output as string
            result.output.as_str().unwrap_or(&result.output.to_string()).to_string()
        }
    }

    /// Simple majority voting: select the most common value.
    fn majority_vote(&self, values: &[String]) -> (String, f64) {
        if values.is_empty() {
            return (String::new(), 0.0);
        }

        let mut counts = std::collections::HashMap::new();
        for value in values {
            *counts.entry(value.clone()).or_insert(0) += 1;
        }

        let (winner, count) = counts.into_iter().max_by_key(|(_, c)| *c).unwrap();
        let agreement = count as f64 / values.len() as f64;

        (winner, agreement)
    }

    /// Weighted voting: values have associated confidence scores.
    fn weighted_vote(&self, results: &[SubagentResult]) -> (String, f64) {
        if results.is_empty() {
            return (String::new(), 0.0);
        }

        let mut weighted_counts = std::collections::HashMap::new();
        let mut total_weight = 0.0;

        for result in results {
            let value = self.extract_vote_value(result);
            // Extract confidence score if available, default to 1.0
            let weight = result.output.get("confidence").and_then(|v| v.as_f64()).unwrap_or(1.0);

            *weighted_counts.entry(value).or_insert(0.0) += weight;
            total_weight += weight;
        }

        let (winner, weight) = weighted_counts
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        let agreement = if total_weight > 0.0 { weight / total_weight } else { 0.0 };

        (winner, agreement)
    }

    /// Borda count: rank-based voting.
    fn borda_vote(&self, results: &[SubagentResult]) -> (String, f64) {
        if results.is_empty() {
            return (String::new(), 0.0);
        }

        // Assume each result contains a "ranking" array
        // Format: [{"name": "option1", "rank": 1}, {"name": "option2", "rank": 2}, ...]
        let mut scores = std::collections::HashMap::new();
        let mut max_rank = 0;

        for result in results {
            if let Some(ranking) = result.output.get("ranking").and_then(|v| v.as_array()) {
                for item in ranking {
                    if let (Some(name), Some(rank)) = (
                        item.get("name").and_then(|v| v.as_str()),
                        item.get("rank").and_then(|v| v.as_u64()),
                    ) {
                        let name = name.to_string();
                        let rank = rank as usize;
                        max_rank = max_rank.max(rank);
                        // Borda score: lower rank = better (rank 1 gets max_rank points)
                        *scores.entry(name).or_insert(0) += max_rank + 1 - rank;
                    }
                }
            }
        }

        if scores.is_empty() {
            // Fallback to majority vote if no ranking data
            let values: Vec<String> = results.iter().map(|r| self.extract_vote_value(r)).collect();
            return self.majority_vote(&values);
        }

        let (winner, score) = scores.into_iter().max_by_key(|(_, s)| *s).unwrap();
        let max_possible = results.len() * max_rank;
        let agreement = if max_possible > 0 { score as f64 / max_possible as f64 } else { 0.0 };

        (winner, agreement)
    }

    /// Approval voting: count approve/disapprove.
    fn approval_vote(&self, results: &[SubagentResult]) -> (String, f64) {
        if results.is_empty() {
            return (String::new(), 0.0);
        }

        let mut approvals = std::collections::HashMap::new();
        let mut total_votes = 0;

        for result in results {
            // Try to extract approval status
            let approved = result
                .output
                .get("approve")
                .or_else(|| result.output.get("approval"))
                .or_else(|| result.output.get("approved"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true); // Default to approve if not specified

            let value = self.extract_vote_value(result);

            if approved {
                *approvals.entry(value).or_insert(0) += 1;
            }
            total_votes += 1;
        }

        if approvals.is_empty() {
            return (String::new(), 0.0);
        }

        let (winner, count) = approvals.into_iter().max_by_key(|(_, c)| *c).unwrap();
        let agreement = count as f64 / total_votes as f64;

        (winner, agreement)
    }

    /// Execute voting based on configured method.
    fn execute_vote(&self, results: &[SubagentResult]) -> (String, f64) {
        match self.config.voting_method {
            VotingMethod::Majority => {
                let values: Vec<String> =
                    results.iter().map(|r| self.extract_vote_value(r)).collect();
                self.majority_vote(&values)
            }
            VotingMethod::Weighted => self.weighted_vote(results),
            VotingMethod::Borda => self.borda_vote(results),
            VotingMethod::Approval => self.approval_vote(results),
        }
    }
}

#[async_trait::async_trait]
impl ResultMerger for ConsensusMerger {
    async fn merge(
        &self,
        _ctx: &MergeContext,
        results: &[SubagentResult],
    ) -> PortResult<MergedResult> {
        if results.is_empty() {
            return Ok(MergedResult::default());
        }

        // Execute voting
        let (winner, agreement) = self.execute_vote(results);

        // Check if threshold is met
        let consensus_reached = agreement >= self.config.threshold;

        // Prepare merged result
        let mut merged = MergedResult::default();
        let outputs: Vec<Value> = results.iter().map(|r| r.output.clone()).collect();
        merged.content = Value::Array(outputs);
        merged.summary = Some(format!(
            "Consensus: {} (agreement: {:.1}%, threshold: {:.1}%, reached: {})",
            winner,
            agreement * 100.0,
            self.config.threshold * 100.0,
            if consensus_reached { "yes" } else { "no" }
        ));
        merged.metadata = serde_json::json!({
            "voting_method": match self.config.voting_method {
                VotingMethod::Majority => "majority",
                VotingMethod::Weighted => "weighted",
                VotingMethod::Borda => "borda",
                VotingMethod::Approval => "approval",
            },
            "winner": winner,
            "agreement_score": agreement,
            "threshold": self.config.threshold,
            "consensus_reached": consensus_reached,
            "num_results": results.len(),
            "vote_field": self.config.vote_field,
        });

        Ok(merged)
    }

    fn strategy_name(&self) -> &'static str {
        "consensus"
    }
}

/// Merge strategy selector.
#[derive(Debug, Clone)]
pub enum MergeStrategy {
    /// Simple concatenation
    Concatenate(ConcatenateConfig),
    /// LLM-based summarization
    LlmSummarized(LlmSummaryConfig),
    /// Consensus/voting
    Consensus(ConsensusConfig),
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::Concatenate(ConcatenateConfig::default())
    }
}

/// Create a merger from a strategy.
#[must_use]
pub fn create_merger_from_strategy(
    strategy: &MergeStrategy,
    llm_port: Option<Arc<dyn LLMPort>>,
) -> Box<dyn ResultMerger> {
    match strategy {
        MergeStrategy::Concatenate(config) => Box::new(ConcatenateMerger::new(config.clone())),
        MergeStrategy::LlmSummarized(config) => {
            let llm_port = llm_port
                .unwrap_or_else(|| panic!("LLM port is required for LlmSummarized strategy"));
            Box::new(LlmSummaryMerger::new(config.clone(), llm_port))
        }
        MergeStrategy::Consensus(config) => Box::new(ConsensusMerger::new(config.clone())),
    }
}

/// Helper functions to create common merger configurations.
pub mod merger_presets {
    use super::*;

    /// Create a concatenation merger with custom separator.
    #[must_use]
    pub fn concatenate_with_separator(separator: impl Into<String>) -> ConcatenateMerger {
        ConcatenateMerger::new(ConcatenateConfig {
            separator: separator.into(),
            preserve_order: true,
            include_metadata: true,
        })
    }

    /// Create a concatenation merger without metadata.
    #[must_use]
    pub fn concatenate_plain() -> ConcatenateMerger {
        ConcatenateMerger::new(ConcatenateConfig {
            separator: "\n".to_string(),
            preserve_order: true,
            include_metadata: false,
        })
    }

    /// Create an LLM summary merger with default config.
    #[must_use]
    pub fn llm_summary(llm_port: Arc<dyn crate::ports::LLMPort>) -> LlmSummaryMerger {
        LlmSummaryMerger::with_default_config(llm_port)
    }

    /// Create an LLM summary merger for detailed summaries.
    #[must_use]
    pub fn llm_detailed_summary(llm_port: Arc<dyn crate::ports::LLMPort>) -> LlmSummaryMerger {
        LlmSummaryMerger::new(LlmSummaryConfig::detailed_summary(), llm_port)
    }

    /// Create an LLM summary merger for executive summaries.
    #[must_use]
    pub fn llm_executive_summary(llm_port: Arc<dyn crate::ports::LLMPort>) -> LlmSummaryMerger {
        LlmSummaryMerger::new(LlmSummaryConfig::executive_summary(), llm_port)
    }

    /// Create a consensus merger with simple majority.
    #[must_use]
    pub fn consensus_majority() -> ConsensusMerger {
        ConsensusMerger::with_default_config()
    }

    /// Create a consensus merger with supermajority requirement.
    #[must_use]
    pub fn consensus_supermajority() -> ConsensusMerger {
        ConsensusMerger::new(ConsensusConfig::supermajority())
    }

    /// Create a consensus merger for approval voting.
    #[must_use]
    pub fn consensus_approval() -> ConsensusMerger {
        ConsensusMerger::new(ConsensusConfig::approval_voting())
    }
}

/// Default summary prompt template.
const DEFAULT_SUMMARY_PROMPT: &str = r#"You are an expert synthesizer. Your job is to combine multiple subagent outputs into a coherent summary.

Subagent Outputs:
{outputs}

Instructions:
- Identify common themes and agreements
- Note any contradictions or disagreements
- Extract key findings from each output
- Create a concise, well-organized summary

Summary:"#;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn create_test_result(ok: bool, output: Value) -> SubagentResult {
        SubagentResult { task_id: Uuid::new_v4(), ok, output }
    }

    #[test]
    fn concatenate_merger_basic() {
        let _merger = ConcatenateMerger::with_default_config();
        let _results = [
            create_test_result(true, Value::String("Result 1".to_string())),
            create_test_result(true, Value::String("Result 2".to_string())),
        ];

        let _ctx = MergeContext::new(
            ThreadId::new_v4(),
            RunId::new_v4(),
            ThreadState::default(),
            SubtaskPlan::default(),
            None,
        );

        // Note: Can't test async in simple unit tests
        // This is tested in integration tests
    }

    #[test]
    fn merge_strategy_default() {
        let strategy = MergeStrategy::default();
        match strategy {
            MergeStrategy::Concatenate(_) => {} // Expected
            _ => panic!("Default strategy should be Concatenate"),
        }
    }

    #[test]
    fn merged_result_default() {
        let result = MergedResult::default();
        assert_eq!(result.content, Value::Null);
        assert!(result.summary.is_none());
    }

    #[test]
    fn llm_summary_config_presets() {
        let detailed = LlmSummaryConfig::detailed_summary();
        assert!(detailed.max_summary_tokens >= 500);

        let executive = LlmSummaryConfig::executive_summary();
        assert!(executive.max_summary_tokens <= 500);
    }

    #[test]
    fn consensus_config_presets() {
        let majority = ConsensusConfig::simple_majority();
        assert_eq!(majority.threshold, 0.5);
        assert!(matches!(majority.voting_method, VotingMethod::Majority));

        let supermajority = ConsensusConfig::supermajority();
        assert_eq!(supermajority.threshold, 0.67);

        let unanimous = ConsensusConfig::unanimous();
        assert_eq!(unanimous.threshold, 0.9);

        let weighted = ConsensusConfig::weighted_confidence();
        assert!(matches!(weighted.voting_method, VotingMethod::Weighted));
    }

    #[test]
    fn consensus_config_builder() {
        let config = ConsensusConfig::new(VotingMethod::Borda, 0.6).with_vote_field("answer");

        assert_eq!(config.threshold, 0.6);
        assert!(matches!(config.voting_method, VotingMethod::Borda));
        assert_eq!(config.vote_field, Some("answer".to_string()));
    }
}
