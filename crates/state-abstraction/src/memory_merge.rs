//! Cross-thread memory merge engine for unified user profile.

use crate::memory_document::{Fact, MemoryDocument};
use crate::memory_prompt::{self, MERGE_PROFILE_PROMPT};
use crate::memory_voting::MemoryVotingEngine;
use agent_ports::{LLMPort, LlmTurnContext, ThreadId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unified user profile merged from multiple threads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: String,
    pub thread_ids: Vec<ThreadId>,
    pub merged_facts: Vec<Fact>,
    pub profile_summary: String,
    pub key_themes: Vec<String>,
    pub last_merged_at: DateTime<Utc>,
    pub confidence: f32,
}

impl UserProfile {
    /// Create a new user profile.
    pub fn new(user_id: String) -> Self {
        Self {
            user_id,
            thread_ids: Vec::new(),
            merged_facts: Vec::new(),
            profile_summary: String::new(),
            key_themes: Vec::new(),
            last_merged_at: Utc::now(),
            confidence: 0.0,
        }
    }
}

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub profile: UserProfile,
    pub facts_merged: usize,
    pub facts_removed: usize,
    pub conflicts_resolved: usize,
}

/// Memory merge engine for consolidating multiple thread memories.
pub struct MemoryMergeEngine {
    llm_port: Box<dyn LLMPort>,
    voting_engine: MemoryVotingEngine,
}

impl MemoryMergeEngine {
    /// Create a new merge engine.
    pub fn new(llm_port: Box<dyn LLMPort>, voting_engine: MemoryVotingEngine) -> Self {
        Self { llm_port, voting_engine }
    }

    /// Merge multiple thread memories into a unified profile.
    pub async fn merge_threads(
        &self,
        user_id: String,
        thread_memories: &[(ThreadId, MemoryDocument)],
        existing_profile: Option<UserProfile>,
    ) -> Result<MergeResult, String> {
        if thread_memories.is_empty() {
            return Err("No thread memories to merge".to_string());
        }

        // Step 1: Collect all facts
        let all_facts: Vec<Fact> =
            thread_memories.iter().flat_map(|(_, doc)| doc.facts.clone()).collect();

        // Step 2: Resolve conflicts using voting
        let (resolved_facts, conflicts_resolved) = self.resolve_all_conflicts(all_facts);

        // Step 3: Call LLM to summarize profile
        let (profile_summary, key_themes, confidence) =
            self.llm_summarize_profile(&resolved_facts).await?;

        // Step 4: Build UserProfile
        let mut profile = existing_profile.unwrap_or_else(|| UserProfile::new(user_id));
        profile.thread_ids = thread_memories.iter().map(|(tid, _)| *tid).collect();
        profile.merged_facts = resolved_facts;
        profile.profile_summary = profile_summary;
        profile.key_themes = key_themes;
        profile.last_merged_at = Utc::now();
        profile.confidence = confidence;

        let facts_merged = profile.merged_facts.len();
        let facts_removed =
            thread_memories.iter().map(|(_, d)| d.facts.len()).sum::<usize>() - facts_merged;

        Ok(MergeResult { profile, facts_merged, facts_removed, conflicts_resolved })
    }

    /// Resolve conflicts across all facts.
    fn resolve_all_conflicts(&self, facts: Vec<Fact>) -> (Vec<Fact>, usize) {
        if facts.is_empty() {
            return (vec![], 0);
        }

        // Group facts by potential conflict
        let mut conflict_groups: Vec<Vec<Fact>> = Vec::new();
        let mut assigned: Vec<bool> = vec![false; facts.len()];

        for (i, fact) in facts.iter().enumerate() {
            if assigned[i] {
                continue;
            }

            let mut group = vec![fact.clone()];
            assigned[i] = true;

            for (j, other) in facts.iter().enumerate() {
                if !assigned[j] && self.voting_engine.detect_conflict(fact, other) {
                    group.push(other.clone());
                    assigned[j] = true;
                }
            }

            conflict_groups.push(group);
        }

        // Resolve each conflict group
        let mut resolved_facts = Vec::new();
        let mut conflicts_resolved = 0;

        for group in conflict_groups {
            if group.len() == 1 {
                resolved_facts.push(group.into_iter().next().unwrap());
            } else {
                let result = self.voting_engine.resolve_conflict(&group);
                if let Some(winner) = result.winning_fact {
                    resolved_facts.push(winner);
                    conflicts_resolved += 1;
                }
            }
        }

        (resolved_facts, conflicts_resolved)
    }

    /// Call LLM to summarize profile from facts.
    async fn llm_summarize_profile(
        &self,
        facts: &[Fact],
    ) -> Result<(String, Vec<String>, f32), String> {
        let facts_str = memory_prompt::format_facts(facts);
        let prompt = MERGE_PROFILE_PROMPT.replace("{facts}", &facts_str);

        let response = self
            .llm_port
            .infer_turn(LlmTurnContext {
                messages: vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })],
                ..Default::default()
            })
            .await
            .map_err(|e| format!("LLM call failed: {}", e))?;

        let response_text =
            response.assistant_text.ok_or_else(|| "No response from LLM".to_string())?;

        // Parse JSON response
        let summary_result: MergeSummaryResult = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        Ok((summary_result.profile_summary, summary_result.key_themes, summary_result.confidence))
    }
}

/// LLM merge summary result.
#[derive(Debug, Clone, Deserialize)]
struct MergeSummaryResult {
    profile_summary: String,
    key_themes: Vec<String>,
    confidence: f32,
}

#[cfg(test)]
#[allow(clippy::all)]
mod tests {
    use super::*;
    use crate::memory_document::{Fact, FactCategory};

    #[test]
    fn test_user_profile_creation() {
        let profile = UserProfile::new("user-123".to_string());
        assert_eq!(profile.user_id, "user-123");
        assert!(profile.thread_ids.is_empty());
        assert!(profile.merged_facts.is_empty());
    }

    #[test]
    fn test_resolve_all_conflicts() {
        let voting_engine = MemoryVotingEngine::with_defaults();
        let engine =
            MemoryMergeEngine::new(Box::new(test_utils::MockLLMPort::new()), voting_engine);

        let facts = vec![
            Fact::new("Fact 1".to_string(), FactCategory::Knowledge, 0.9, "t1".to_string()),
            Fact::new("Fact 2".to_string(), FactCategory::Knowledge, 0.5, "t2".to_string()),
        ];

        let (resolved, _conflicts) = engine.resolve_all_conflicts(facts);
        // Resolution may merge facts, so just verify we get some results
        assert!(!resolved.is_empty());
    }
}

// Test utilities module
#[cfg(test)]
pub mod test_utils {
    use agent_ports::{LLMPort, LlmTurnContext, LlmTurnOutput, PortResult};
    use async_trait::async_trait;

    pub struct MockLLMPort;

    impl Default for MockLLMPort {
        fn default() -> Self {
            Self
        }
    }

    impl MockLLMPort {
        pub fn new() -> Self {
            Self
        }
    }

    #[async_trait]
    impl LLMPort for MockLLMPort {
        async fn infer_turn(&self, _ctx: LlmTurnContext) -> PortResult<LlmTurnOutput> {
            Ok(LlmTurnOutput {
                assistant_text: Some(
                    r#"{
                    "profile_summary": "Test profile",
                    "key_themes": ["theme1"],
                    "confidence": 0.9
                }"#
                    .to_string(),
                ),
                ..Default::default()
            })
        }
    }
}
