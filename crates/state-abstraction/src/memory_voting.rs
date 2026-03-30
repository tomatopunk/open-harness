//! Confidence-weighted voting system for fact conflict resolution.

use crate::memory::config::{ConflictDetectionConfig, VotingConfig};
use crate::memory_document::Fact;
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// A fact with computed voting weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactVote {
    pub fact: Fact,
    pub weight: f32,
}

/// Result of a voting decision.
#[derive(Debug, Clone)]
pub struct VoteResult {
    pub winning_fact: Option<Fact>,
    pub all_candidates: Vec<FactVote>,
    pub decision_reason: String,
}

/// Ranked fact for prioritization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedFact {
    pub fact: Fact,
    pub rank: usize,
    pub weight: f32,
}

/// Voting engine for conflict resolution and fact prioritization.
#[derive(Debug, Clone)]
pub struct MemoryVotingEngine {
    voting_config: VotingConfig,
    conflict_config: ConflictDetectionConfig,
}

impl MemoryVotingEngine {
    /// Create a new voting engine with configuration.
    pub fn new(voting_config: VotingConfig, conflict_config: ConflictDetectionConfig) -> Self {
        Self { voting_config, conflict_config }
    }

    /// Create with default parameters.
    pub fn with_defaults() -> Self {
        Self::new(VotingConfig::default(), ConflictDetectionConfig::default())
    }

    /// Compute voting weight for a fact.
    /// Formula: weight = confidence * recency_factor * source_reliability
    #[must_use]
    pub fn compute_weight(&self, fact: &Fact) -> f32 {
        let days_since_created = (Utc::now() - fact.created_at).num_seconds() as f32 / 86400.0;
        let recency_factor = (-days_since_created / self.voting_config.half_life_days).exp();
        fact.confidence * recency_factor * self.voting_config.source_reliability
    }

    /// Rank facts by weight (descending).
    #[must_use]
    pub fn rank_facts(&self, facts: &[Fact]) -> Vec<RankedFact> {
        let mut votes: Vec<FactVote> = facts
            .iter()
            .map(|fact| FactVote { weight: self.compute_weight(fact), fact: fact.clone() })
            .collect();

        // Sort by weight descending
        votes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

        // Convert to ranked facts
        votes
            .into_iter()
            .enumerate()
            .map(|(i, vote)| RankedFact { fact: vote.fact, rank: i + 1, weight: vote.weight })
            .collect()
    }

    /// Detect if two facts conflict.
    #[must_use]
    pub fn detect_conflict(&self, fact1: &Fact, fact2: &Fact) -> bool {
        // Rule 1: Negation detection
        if self.has_negation_conflict(&fact1.content, &fact2.content) {
            return true;
        }

        // Rule 2: Mutually exclusive values
        if self.has_mutually_exclusive_values(&fact1.content, &fact2.content) {
            return true;
        }

        // Rule 3: Same category and similar content (potential conflict)
        if fact1.category == fact2.category {
            let similarity = self.content_similarity(&fact1.content, &fact2.content);
            if similarity > self.conflict_config.similarity_threshold {
                return true;
            }
        }

        false
    }

    /// Resolve conflict between facts using voting.
    #[must_use]
    pub fn resolve_conflict(&self, facts: &[Fact]) -> VoteResult {
        if facts.is_empty() {
            return VoteResult {
                winning_fact: None,
                all_candidates: vec![],
                decision_reason: "No facts to resolve".to_string(),
            };
        }

        // Compute weights
        let mut votes: Vec<FactVote> = facts
            .iter()
            .map(|fact| FactVote { weight: self.compute_weight(fact), fact: fact.clone() })
            .collect();

        // Sort by weight descending
        votes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));

        let winner = votes.first().cloned();
        let reason = match &winner {
            Some(v) => format!(
                "Fact '{}' wins with weight {:.3} (confidence: {:.2}, recency: {:.2})",
                v.fact.content,
                v.weight,
                v.fact.confidence,
                (-((Utc::now() - v.fact.created_at).num_seconds() as f32 / 86400.0)
                    / self.voting_config.half_life_days)
                    .exp()
            ),
            None => "No winner".to_string(),
        };

        VoteResult {
            winning_fact: winner.map(|v| v.fact),
            all_candidates: votes,
            decision_reason: reason,
        }
    }

    /// Check for negation conflicts.
    fn has_negation_conflict(&self, content1: &str, content2: &str) -> bool {
        let negation_words = &self.conflict_config.negation_words;

        let has_negation =
            |text: &str| -> bool { negation_words.iter().any(|word| text.contains(word)) };

        // Simple heuristic: if one has negation and the other doesn't, and they share key terms
        let neg1 = has_negation(content1);
        let neg2 = has_negation(content2);

        if neg1 != neg2 {
            // Check if they share significant terms
            let similarity = self.content_similarity(content1, content2);
            return similarity > self.conflict_config.similarity_threshold;
        }

        false
    }

    /// Check for mutually exclusive values.
    fn has_mutually_exclusive_values(&self, content1: &str, content2: &str) -> bool {
        // Location conflicts
        let locations = &self.conflict_config.location_keywords;

        let loc1 = locations.iter().find(|loc| content1.contains(*loc));
        let loc2 = locations.iter().find(|loc| content2.contains(*loc));

        if loc1.is_some() && loc2.is_some() && loc1 != loc2 {
            return true;
        }

        false
    }

    /// Simple content similarity (word overlap).
    fn content_similarity(&self, text1: &str, text2: &str) -> f32 {
        let words1: std::collections::HashSet<&str> = text1.split_whitespace().collect();
        let words2: std::collections::HashSet<&str> = text2.split_whitespace().collect();

        if words1.is_empty() || words2.is_empty() {
            return 0.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f32 / union as f32
    }
}

impl Default for MemoryVotingEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_document::FactCategory;

    #[test]
    fn test_compute_weight() {
        let engine = MemoryVotingEngine::with_defaults();
        let fact =
            Fact::new("Test".to_string(), FactCategory::Knowledge, 0.9, "thread-1".to_string());

        let weight = engine.compute_weight(&fact);
        assert!(weight > 0.0);
        assert!(weight <= 0.9); // Should be <= confidence due to recency decay
    }

    #[test]
    fn test_rank_facts() {
        let engine = MemoryVotingEngine::with_defaults();
        let facts = vec![
            Fact::new("Low".to_string(), FactCategory::Knowledge, 0.3, "t1".to_string()),
            Fact::new("High".to_string(), FactCategory::Knowledge, 0.9, "t2".to_string()),
            Fact::new("Medium".to_string(), FactCategory::Knowledge, 0.6, "t3".to_string()),
        ];

        let ranked = engine.rank_facts(&facts);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].rank, 1);
        assert!(ranked[0].fact.content == "High");
    }

    #[test]
    fn test_detect_negation_conflict() {
        let engine = MemoryVotingEngine::with_defaults();

        let fact1 = Fact::new(
            "用户喜欢 Python".to_string(),
            FactCategory::Preference,
            0.9,
            "t1".to_string(),
        );
        let fact2 = Fact::new(
            "用户不喜欢 Python".to_string(),
            FactCategory::Preference,
            0.9,
            "t2".to_string(),
        );

        assert!(engine.detect_conflict(&fact1, &fact2));
    }

    #[test]
    fn test_resolve_conflict() {
        let engine = MemoryVotingEngine::with_defaults();
        let facts = vec![
            Fact::new("Low confidence".to_string(), FactCategory::Knowledge, 0.3, "t1".to_string()),
            Fact::new(
                "High confidence".to_string(),
                FactCategory::Knowledge,
                0.9,
                "t2".to_string(),
            ),
        ];

        let result = engine.resolve_conflict(&facts);
        assert!(result.winning_fact.is_some());
        assert_eq!(result.winning_fact.unwrap().content, "High confidence");
    }
}
