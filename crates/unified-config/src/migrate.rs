//! Configuration migration utilities.
//!
//! This module provides tools for migrating from legacy configuration formats
//! to the unified configuration model.

use crate::{loader::load_unified_config, UnifiedConfig};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Configuration loader error: {0}")]
    Loader(#[from] crate::loader::ConfigLoaderError),

    #[error("Migration validation failed: {0}")]
    ValidationFailed(String),
}

/// Migration result containing the new config and any warnings.
#[derive(Debug, Clone)]
pub struct MigrationResult {
    pub unified_config: UnifiedConfig,
    pub warnings: Vec<String>,
    pub info: MigrationInfo,
}

/// Information about the migration.
#[derive(Debug, Clone)]
pub struct MigrationInfo {
    pub models_migrated: usize,
    pub tools_migrated: usize,
    pub policies_migrated: bool,
    pub subagents_migrated: bool,
}

/// Migrate from legacy configuration to unified configuration.
pub fn migrate_from_legacy(
    app_cfg: &crate::loader::AppConfigRef,
    governance_root: &str,
) -> Result<MigrationResult, MigrationError> {
    let mut warnings = Vec::new();

    // Load unified configuration
    let unified_config = load_unified_config(app_cfg, governance_root)?;

    // Collect migration information
    let info = MigrationInfo {
        models_migrated: unified_config.models.entries.len(),
        tools_migrated: unified_config.tools.manifests.len(),
        policies_migrated: true,
        subagents_migrated: true,
    };

    // Generate warnings for potential issues
    if unified_config.models.entries.is_empty() {
        warnings.push("No models configured in legacy configuration".to_string());
    }

    if unified_config.tools.manifests.is_empty() {
        warnings.push("No tools configured in governance/tools.yaml".to_string());
    }

    if unified_config.models.default_model.is_empty() && !unified_config.models.entries.is_empty() {
        warnings.push(format!(
            "No default_model set, will use first model: {}",
            unified_config.models.entries.first().map(|e| e.name.as_str()).unwrap_or("none")
        ));
    }

    Ok(MigrationResult { unified_config, warnings, info })
}

/// Save unified configuration to a YAML file.
pub fn save_unified_config(
    config: &UnifiedConfig,
    output_path: &Path,
) -> Result<(), MigrationError> {
    let yaml = serde_yaml::to_string(config)?;
    std::fs::write(output_path, yaml)?;
    Ok(())
}

/// Load unified configuration from a YAML file.
pub fn load_unified_config_from_file(path: &Path) -> Result<UnifiedConfig, MigrationError> {
    let content = std::fs::read_to_string(path)?;
    let config: UnifiedConfig = serde_yaml::from_str(&content)?;
    Ok(config)
}

/// Validate a unified configuration.
pub fn validate_config(config: &UnifiedConfig) -> Result<(), MigrationError> {
    // Check for required fields
    if config.models.entries.is_empty() {
        return Err(MigrationError::ValidationFailed(
            "At least one model must be configured".to_string(),
        ));
    }

    // Check for duplicate model names
    let mut model_names = std::collections::HashSet::new();
    for model in &config.models.entries {
        if !model_names.insert(&model.name) {
            return Err(MigrationError::ValidationFailed(format!(
                "Duplicate model name: {}",
                model.name
            )));
        }
    }

    // Check for duplicate tool names
    let mut tool_names = std::collections::HashSet::new();
    for tool in &config.tools.manifests {
        if !tool_names.insert(&tool.name) {
            return Err(MigrationError::ValidationFailed(format!(
                "Duplicate tool name: {}",
                tool.name
            )));
        }
    }

    // Validate policy constraints
    if config.policies.max_turns == 0 {
        return Err(MigrationError::ValidationFailed(
            "max_turns must be greater than 0".to_string(),
        ));
    }

    if config.subagents.max_concurrent == 0 {
        return Err(MigrationError::ValidationFailed(
            "max_concurrent subagents must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

/// Compare legacy and unified configurations to verify migration correctness.
pub fn compare_configs(
    legacy_cfg: &crate::loader::AppConfigRef,
    unified_cfg: &UnifiedConfig,
) -> ComparisonResult {
    let mut result = ComparisonResult::default();

    // Compare model counts
    result.models_legacy = legacy_cfg.models.len();
    result.models_unified = unified_cfg.models.entries.len();
    result.models_match = result.models_legacy == result.models_unified;

    // Check if all legacy models are present in unified config
    let legacy_names: std::collections::HashSet<_> =
        legacy_cfg.models.iter().map(|m| &m.name).collect();
    let unified_names: std::collections::HashSet<_> =
        unified_cfg.models.entries.iter().map(|m| &m.name).collect();

    result.models_missing_in_unified =
        legacy_names.difference(&unified_names).map(|s| s.to_string()).collect();
    result.models_extra_in_unified =
        unified_names.difference(&legacy_names).map(|s| s.to_string()).collect();
    result.models_fully_migrated = result.models_missing_in_unified.is_empty();

    result
}

/// Result of comparing legacy and unified configurations.
#[derive(Debug, Clone, Default)]
pub struct ComparisonResult {
    pub models_legacy: usize,
    pub models_unified: usize,
    pub models_match: bool,
    pub models_fully_migrated: bool,
    pub models_missing_in_unified: Vec<String>,
    pub models_extra_in_unified: Vec<String>,
}

impl ComparisonResult {
    pub fn is_successful(&self) -> bool {
        self.models_fully_migrated && self.models_match
    }

    pub fn summary(&self) -> String {
        if self.is_successful() {
            format!("Migration successful: {} models migrated", self.models_unified)
        } else {
            let mut summary = format!(
                "Migration partial: {} legacy models, {} unified models\n",
                self.models_legacy, self.models_unified
            );

            if !self.models_missing_in_unified.is_empty() {
                summary.push_str(&format!(
                    "  Missing in unified: {:?}\n",
                    self.models_missing_in_unified
                ));
            }

            if !self.models_extra_in_unified.is_empty() {
                summary
                    .push_str(&format!("  Extra in unified: {:?}\n", self.models_extra_in_unified));
            }

            summary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_config_success() {
        let config = UnifiedConfig {
            models: crate::ModelRegistry {
                default_model: "gpt-4".to_string(),
                entries: vec![crate::ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "langchain_openai:ChatOpenAI".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: crate::ModelConfig::default(),
                }],
            },
            tools: crate::ToolRegistry::default(),
            subagents: crate::SubagentConfig::default(),
            policies: crate::PolicySwitches::default(),
            acp_agents: crate::ACPAgentsConfig::default(),
        };

        let result = validate_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_config_no_models() {
        let config = UnifiedConfig::default();
        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("At least one model"));
    }

    #[test]
    fn test_validate_config_duplicate_models() {
        let config = UnifiedConfig {
            models: crate::ModelRegistry {
                default_model: "gpt-4".to_string(),
                entries: vec![
                    crate::ModelEntry {
                        name: "gpt-4".to_string(),
                        display_name: "GPT-4".to_string(),
                        provider: "langchain_openai:ChatOpenAI".to_string(),
                        model_id: "gpt-4".to_string(),
                        config: crate::ModelConfig::default(),
                    },
                    crate::ModelEntry {
                        name: "gpt-4".to_string(), // Duplicate
                        display_name: "GPT-4 Again".to_string(),
                        provider: "langchain_openai:ChatOpenAI".to_string(),
                        model_id: "gpt-4".to_string(),
                        config: crate::ModelConfig::default(),
                    },
                ],
            },
            ..Default::default()
        };

        let result = validate_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate model name"));
    }

    #[test]
    fn test_compare_configs() {
        use crate::loader::{AppConfigRef, ModelConfigRef};

        let legacy = AppConfigRef {
            models: vec![
                ModelConfigRef {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    use_provider: "langchain_openai:ChatOpenAI".to_string(),
                    model: "gpt-4".to_string(),
                    api_key: None,
                    max_tokens: None,
                    temperature: None,
                    base_url: None,
                    use_responses_api: None,
                    output_version: None,
                },
                ModelConfigRef {
                    name: "gpt-3.5".to_string(),
                    display_name: "GPT-3.5".to_string(),
                    use_provider: "langchain_openai:ChatOpenAI".to_string(),
                    model: "gpt-3.5-turbo".to_string(),
                    api_key: None,
                    max_tokens: None,
                    temperature: None,
                    base_url: None,
                    use_responses_api: None,
                    output_version: None,
                },
            ],
            extensions_config_path: None,
        };

        let unified = UnifiedConfig {
            models: crate::ModelRegistry {
                default_model: "gpt-4".to_string(),
                entries: vec![crate::ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "langchain_openai:ChatOpenAI".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: crate::ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let comparison = compare_configs(&legacy, &unified);
        assert!(!comparison.models_fully_migrated);
        assert_eq!(comparison.models_missing_in_unified.len(), 1);
        assert_eq!(comparison.models_missing_in_unified[0], "gpt-3.5");
    }
}
