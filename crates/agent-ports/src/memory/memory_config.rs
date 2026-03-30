//! Memory system configuration (DeerFlow-inspired).

use serde::{Deserialize, Serialize};

/// Memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Debounce time in seconds (default: 30).
    /// Conversations are batched within this window to reduce LLM calls.
    #[serde(default = "default_debounce_seconds")]
    pub debounce_seconds: u64,

    /// Minimum confidence threshold for facts (default: 0.7).
    /// Facts below this threshold are discarded.
    #[serde(default = "default_confidence_threshold")]
    pub fact_confidence_threshold: f32,

    /// Maximum number of facts to keep per thread (default: 100).
    /// Older/low-weight facts are pruned when exceeded.
    #[serde(default = "default_max_facts")]
    pub max_facts: usize,

    /// LLM model name for memory updates (default: use system default).
    #[serde(default)]
    pub model_name: Option<String>,

    /// Whether to inject memory into LLM prompts (default: true).
    #[serde(default = "default_true")]
    pub injection_enabled: bool,

    /// Maximum tokens for memory injection (default: 2000).
    #[serde(default = "default_max_injection_tokens")]
    pub max_injection_tokens: usize,

    /// Half-life in days for recency decay in voting (default: 30).
    #[serde(default = "default_half_life_days")]
    pub half_life_days: f32,

    /// Source reliability for facts extracted from user statements (default: 1.0).
    #[serde(default = "default_source_reliability")]
    pub source_reliability: f32,

    /// Interval in hours for background memory merge (default: 24).
    #[serde(default = "default_merge_interval_hours")]
    pub merge_interval_hours: u64,

    /// Whether to enable automatic cross-thread memory merge (default: false).
    #[serde(default)]
    pub auto_merge_enabled: bool,
}

fn default_debounce_seconds() -> u64 {
    30
}

fn default_confidence_threshold() -> f32 {
    0.7
}

fn default_max_facts() -> usize {
    100
}

fn default_true() -> bool {
    true
}

fn default_max_injection_tokens() -> usize {
    2000
}

fn default_half_life_days() -> f32 {
    30.0
}

fn default_source_reliability() -> f32 {
    1.0
}

fn default_merge_interval_hours() -> u64 {
    24
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            debounce_seconds: default_debounce_seconds(),
            fact_confidence_threshold: default_confidence_threshold(),
            max_facts: default_max_facts(),
            model_name: None,
            injection_enabled: default_true(),
            max_injection_tokens: default_max_injection_tokens(),
            half_life_days: default_half_life_days(),
            source_reliability: default_source_reliability(),
            merge_interval_hours: default_merge_interval_hours(),
            auto_merge_enabled: false,
        }
    }
}

impl MemoryConfig {
    /// Production defaults (conservative, high quality).
    #[must_use]
    pub fn production() -> Self {
        Self {
            debounce_seconds: 30,
            fact_confidence_threshold: 0.75,
            max_facts: 100,
            model_name: None,
            injection_enabled: true,
            max_injection_tokens: 2000,
            half_life_days: 30.0,
            source_reliability: 1.0,
            merge_interval_hours: 24,
            auto_merge_enabled: true,
        }
    }

    /// Development defaults (faster iteration, lower thresholds).
    #[must_use]
    pub fn development() -> Self {
        Self {
            debounce_seconds: 10,
            fact_confidence_threshold: 0.6,
            max_facts: 50,
            model_name: None,
            injection_enabled: true,
            max_injection_tokens: 1000,
            half_life_days: 15.0,
            source_reliability: 1.0,
            merge_interval_hours: 1,
            auto_merge_enabled: false,
        }
    }

    /// Testing defaults (minimal overhead).
    #[must_use]
    pub fn testing() -> Self {
        Self {
            debounce_seconds: 1,
            fact_confidence_threshold: 0.5,
            max_facts: 20,
            model_name: None,
            injection_enabled: false,
            max_injection_tokens: 500,
            half_life_days: 7.0,
            source_reliability: 1.0,
            merge_interval_hours: 0,
            auto_merge_enabled: false,
        }
    }

    /// Validate configuration and return warnings.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.debounce_seconds < 1 {
            warnings.push("debounce_seconds < 1 may cause excessive LLM calls".to_string());
        } else if self.debounce_seconds > 300 {
            warnings.push("debounce_seconds > 300 may delay memory updates".to_string());
        }

        if self.fact_confidence_threshold < 0.3 {
            warnings
                .push("fact_confidence_threshold < 0.3 may store low-quality facts".to_string());
        } else if self.fact_confidence_threshold > 0.95 {
            warnings.push("fact_confidence_threshold > 0.95 may discard useful facts".to_string());
        }

        if self.max_facts < 10 {
            warnings.push("max_facts < 10 is too restrictive".to_string());
        } else if self.max_facts > 500 {
            warnings.push("max_facts > 500 may impact performance".to_string());
        }

        if self.max_injection_tokens < 100 {
            warnings.push("max_injection_tokens < 100 may truncate memory".to_string());
        } else if self.max_injection_tokens > 8000 {
            warnings.push("max_injection_tokens > 8000 may exceed context limits".to_string());
        }

        if self.half_life_days < 1.0 {
            warnings.push("half_life_days < 1.0 causes very rapid decay".to_string());
        }

        if self.source_reliability < 0.5 || self.source_reliability > 1.0 {
            warnings.push("source_reliability should be between 0.5 and 1.0".to_string());
        }

        warnings
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
        assert!(config.injection_enabled);
        assert_eq!(config.max_injection_tokens, 2000);
    }

    #[test]
    fn test_production_config() {
        let config = MemoryConfig::production();
        assert_eq!(config.fact_confidence_threshold, 0.75);
        assert!(config.auto_merge_enabled);
    }

    #[test]
    fn test_development_config() {
        let config = MemoryConfig::development();
        assert_eq!(config.debounce_seconds, 10);
        assert_eq!(config.fact_confidence_threshold, 0.6);
        assert!(!config.auto_merge_enabled);
    }

    #[test]
    fn test_config_validation() {
        let bad_config = MemoryConfig {
            debounce_seconds: 0,
            fact_confidence_threshold: 0.1,
            max_facts: 5,
            max_injection_tokens: 50,
            half_life_days: 0.5,
            source_reliability: 0.3,
            ..Default::default()
        };

        let warnings = bad_config.validate();
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.contains("debounce")));
        assert!(warnings.iter().any(|w| w.contains("confidence")));
        assert!(warnings.iter().any(|w| w.contains("max_facts")));
    }

    #[test]
    fn test_valid_config() {
        let config = MemoryConfig::production();
        let warnings = config.validate();
        assert!(warnings.is_empty());
    }
}
