//! Unified configuration for memory system.
//!
//! All configurable parameters are centralized here for easy management.

use serde::{Deserialize, Serialize};

/// Main memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Debounce time in seconds (default: 30).
    #[serde(default = "default_debounce_seconds")]
    pub debounce_seconds: u64,

    /// Minimum confidence threshold for facts (default: 0.7).
    #[serde(default = "default_confidence_threshold")]
    pub fact_confidence_threshold: f32,

    /// Maximum number of facts to keep per thread (default: 100).
    #[serde(default = "default_max_facts")]
    pub max_facts: usize,

    /// Maximum tokens for memory injection (default: 2000).
    #[serde(default = "default_max_injection_tokens")]
    pub max_injection_tokens: usize,

    /// Voting configuration.
    #[serde(default)]
    pub voting: VotingConfig,

    /// Conflict detection configuration.
    #[serde(default)]
    pub conflict_detection: ConflictDetectionConfig,

    /// Prompt configuration.
    #[serde(default)]
    pub prompts: PromptConfig,
}

/// Voting engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingConfig {
    /// Conflict threshold (default: 0.3).
    #[serde(default = "default_conflict_threshold")]
    pub conflict_threshold: f32,

    /// Half-life in days for recency decay (default: 30.0).
    #[serde(default = "default_half_life_days")]
    pub half_life_days: f32,

    /// Source reliability multiplier (default: 1.0).
    #[serde(default = "default_source_reliability")]
    pub source_reliability: f32,
}

/// Conflict detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictDetectionConfig {
    /// Negation words for conflict detection.
    #[serde(default = "default_negation_words")]
    pub negation_words: Vec<String>,

    /// Location keywords for mutually exclusive detection.
    #[serde(default = "default_location_keywords")]
    pub location_keywords: Vec<String>,

    /// Similarity threshold for conflict detection (default: 0.3).
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f32,
}

/// Prompt configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptConfig {
    /// Confidence level ranges.
    #[serde(default)]
    pub confidence_levels: ConfidenceLevels,
}

/// Confidence level ranges for fact extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceLevels {
    /// Range for explicitly stated facts (default: 0.9-1.0).
    #[serde(default = "default_explicit_range")]
    pub explicit_range: (f32, f32),

    /// Range for strongly implied facts (default: 0.7-0.8).
    #[serde(default = "default_implied_range")]
    pub implied_range: (f32, f32),

    /// Range for inferred patterns (default: 0.5-0.6).
    #[serde(default = "default_inferred_range")]
    pub inferred_range: (f32, f32),
}

// Default value functions
fn default_debounce_seconds() -> u64 {
    30
}
fn default_confidence_threshold() -> f32 {
    0.7
}
fn default_max_facts() -> usize {
    100
}
fn default_max_injection_tokens() -> usize {
    2000
}
fn default_conflict_threshold() -> f32 {
    0.3
}
fn default_half_life_days() -> f32 {
    30.0
}
fn default_source_reliability() -> f32 {
    1.0
}
fn default_similarity_threshold() -> f32 {
    0.3
}

fn default_negation_words() -> Vec<String> {
    vec![
        "不".to_string(),
        "没".to_string(),
        "无".to_string(),
        "非".to_string(),
        "un".to_string(),
        "not".to_string(),
        "no".to_string(),
        "never".to_string(),
    ]
}

fn default_location_keywords() -> Vec<String> {
    vec![
        "北京".to_string(),
        "上海".to_string(),
        "广州".to_string(),
        "深圳".to_string(),
        "杭州".to_string(),
        "Beijing".to_string(),
        "Shanghai".to_string(),
        "Guangzhou".to_string(),
        "Shenzhen".to_string(),
        "Hangzhou".to_string(),
    ]
}

fn default_explicit_range() -> (f32, f32) {
    (0.9, 1.0)
}
fn default_implied_range() -> (f32, f32) {
    (0.7, 0.8)
}
fn default_inferred_range() -> (f32, f32) {
    (0.5, 0.6)
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            debounce_seconds: default_debounce_seconds(),
            fact_confidence_threshold: default_confidence_threshold(),
            max_facts: default_max_facts(),
            max_injection_tokens: default_max_injection_tokens(),
            voting: VotingConfig {
                conflict_threshold: default_conflict_threshold(),
                half_life_days: default_half_life_days(),
                source_reliability: default_source_reliability(),
            },
            conflict_detection: ConflictDetectionConfig {
                negation_words: default_negation_words(),
                location_keywords: default_location_keywords(),
                similarity_threshold: default_similarity_threshold(),
            },
            prompts: PromptConfig {
                confidence_levels: ConfidenceLevels {
                    explicit_range: default_explicit_range(),
                    implied_range: default_implied_range(),
                    inferred_range: default_inferred_range(),
                },
            },
        }
    }
}

impl Default for VotingConfig {
    fn default() -> Self {
        Self {
            conflict_threshold: default_conflict_threshold(),
            half_life_days: default_half_life_days(),
            source_reliability: default_source_reliability(),
        }
    }
}

impl Default for ConflictDetectionConfig {
    fn default() -> Self {
        Self {
            negation_words: default_negation_words(),
            location_keywords: default_location_keywords(),
            similarity_threshold: default_similarity_threshold(),
        }
    }
}

impl Default for ConfidenceLevels {
    fn default() -> Self {
        Self {
            explicit_range: default_explicit_range(),
            implied_range: default_implied_range(),
            inferred_range: default_inferred_range(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MemoryConfig::default();
        assert_eq!(config.debounce_seconds, 30);
        assert_eq!(config.fact_confidence_threshold, 0.7);
        assert_eq!(config.max_facts, 100);
        assert!(!config.conflict_detection.negation_words.is_empty());
    }

    #[test]
    fn test_voting_config() {
        let config = VotingConfig::default();
        assert_eq!(config.conflict_threshold, 0.3);
        assert_eq!(config.half_life_days, 30.0);
    }

    #[test]
    fn test_conflict_detection_config() {
        let config = ConflictDetectionConfig::default();
        assert!(config.negation_words.contains(&"不".to_string()));
        assert!(config.location_keywords.contains(&"北京".to_string()));
        assert_eq!(config.similarity_threshold, 0.3);
    }
}
