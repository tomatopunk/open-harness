use crate::UnifiedConfig;
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
}
