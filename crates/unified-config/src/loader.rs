//! Configuration loading and merging logic.
//!
//! This module provides functions to load configuration from multiple sources
//! and merge them into a unified configuration.

use crate::{
    ModelConfig, ModelEntry, ModelRegistry, PolicySwitches, SubagentBudget, SubagentConfig,
    ToolAssemblyPolicy, ToolManifest, ToolRegistry, UnifiedConfig,
};
use std::collections::HashSet;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigLoaderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Load unified configuration from AppConfig and governance directory.
/// This function is re-exported from config-runtime with the proper AppConfig type.
pub fn load_unified_config(
    app_cfg: &crate::loader::AppConfigRef,
    governance_root: &str,
) -> Result<UnifiedConfig, ConfigLoaderError> {
    // Load models from config.yaml (complete configuration)
    let models_from_yaml = &app_cfg.models;

    // Load models from governance/models.yaml (simplified entries)
    let models_from_governance = load_governance_models(governance_root)?;

    // Merge model configurations
    let merged_models = merge_model_configs(models_from_yaml, &models_from_governance)?;

    // Load tools from governance/tools.yaml
    let tools = load_governance_tools(governance_root)?;

    // Load policies from governance/policies.yaml
    let policies = load_governance_policies(governance_root)?;

    // Load subagents from governance/subagents.yaml
    let subagents = load_governance_subagents(governance_root)?;

    // Load ACP agents from governance/acp_agents.yaml
    let acp_agents = load_governance_acp_agents(governance_root)?;

    Ok(UnifiedConfig { models: merged_models, tools, subagents, policies, acp_agents })
}

/// Reference to AppConfig fields needed for loading.
/// This avoids circular dependencies by not requiring the full AppConfig type.
pub struct AppConfigRef {
    pub models: Vec<ModelConfigRef>,
}

/// Reference to ModelConfig fields needed for loading.
pub struct ModelConfigRef {
    pub name: String,
    pub display_name: String,
    pub use_provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub base_url: Option<String>,
    pub use_responses_api: Option<bool>,
    pub output_version: Option<String>,
}

/// Load model entries from governance/models.yaml.
fn load_governance_models(
    governance_root: &str,
) -> Result<GovernanceModelsFile, ConfigLoaderError> {
    let path = Path::new(governance_root).join("models.yaml");
    if !path.exists() {
        return Ok(GovernanceModelsFile::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let file: GovernanceModelsFile = serde_yaml::from_str(&content)?;
    Ok(file)
}

/// Governance models file structure (simplified).
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct GovernanceModelsFile {
    #[serde(default)]
    default_model: Option<String>,

    #[serde(default)]
    #[allow(dead_code)]
    entries: Vec<GovernanceModelEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct GovernanceModelEntry {
    name: String,
    #[serde(default)]
    provider: Option<String>,
}

/// Merge model configurations from config.yaml and governance/models.yaml.
fn merge_model_configs(
    yaml_models: &[ModelConfigRef],
    governance_models: &GovernanceModelsFile,
) -> Result<ModelRegistry, ConfigLoaderError> {
    let mut entries = Vec::new();

    // Convert config.yaml models to ModelEntry
    for model_cfg in yaml_models {
        let entry = ModelEntry {
            name: model_cfg.name.clone(),
            display_name: model_cfg.display_name.clone(),
            provider: model_cfg.use_provider.clone(),
            model_id: model_cfg.model.clone(),
            config: ModelConfig {
                api_key: model_cfg.api_key.clone(),
                max_tokens: model_cfg.max_tokens,
                temperature: model_cfg.temperature,
                base_url: model_cfg.base_url.clone(),
                use_responses_api: model_cfg.use_responses_api,
                output_version: model_cfg.output_version.clone(),
                extra: serde_json::Map::new(),
            },
        };
        entries.push(entry);
    }

    // If governance has a default_model, use it
    let default_model = governance_models.default_model.clone().unwrap_or_default();

    Ok(ModelRegistry { default_model, entries })
}

/// Load tool manifests from governance/tools.yaml.
pub fn load_governance_tools(governance_root: &str) -> Result<ToolRegistry, ConfigLoaderError> {
    let path = Path::new(governance_root).join("tools.yaml");
    if !path.exists() {
        return Ok(ToolRegistry::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let file: GovernanceToolsFile = serde_yaml::from_str(&content)?;

    let manifests = file.manifests.into_iter().map(|m| m.into()).collect();

    Ok(ToolRegistry { manifests, assembly_policy: ToolAssemblyPolicy::default() })
}

/// Governance tools file structure.
#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceToolsFile {
    #[serde(default)]
    manifests: Vec<GovernanceToolManifest>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceToolManifest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    capability_tags: Vec<String>,
    #[serde(default)]
    risk_level: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default)]
    retry_max: u32,
    #[serde(default)]
    side_effect_class: String,
    #[serde(default)]
    provider_type: Option<String>,
    #[serde(default)]
    provider_name: Option<String>,
    #[serde(default)]
    load_path: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    input_schema: Option<serde_json::Value>,
}

fn default_timeout_ms() -> u64 {
    30000
}

impl From<GovernanceToolManifest> for ToolManifest {
    fn from(gov: GovernanceToolManifest) -> Self {
        use crate::{RiskLevel, SideEffectClass, ToolProviderType};

        let risk_level = match gov.risk_level.to_lowercase().as_str() {
            "low" => RiskLevel::Low,
            "medium" => RiskLevel::Medium,
            "high" => RiskLevel::High,
            _ => RiskLevel::Low,
        };

        let side_effect_class = match gov.side_effect_class.to_lowercase().as_str() {
            "none" => SideEffectClass::None,
            "read" => SideEffectClass::Read,
            "write" => SideEffectClass::Write,
            "network" => SideEffectClass::Network,
            "exec" => SideEffectClass::Exec,
            _ => SideEffectClass::None,
        };

        let provider_type =
            match gov.provider_type.as_deref().unwrap_or("local").to_lowercase().as_str() {
                "local" => ToolProviderType::Local,
                "mcp" => ToolProviderType::Mcp,
                "skill" => ToolProviderType::Skill,
                "community" => ToolProviderType::Community,
                _ => ToolProviderType::Local,
            };

        Self {
            name: gov.name,
            description: gov.description,
            capability_tags: gov.capability_tags,
            risk_level,
            timeout_ms: gov.timeout_ms,
            retry_max: gov.retry_max,
            side_effect_class,
            provider_type,
            provider_name: gov.provider_name.unwrap_or_default(),
            load_path: gov.load_path,
            version: gov.version,
            input_schema: gov.input_schema,
        }
    }
}

/// Load policies from governance/policies.yaml.
pub fn load_governance_policies(
    governance_root: &str,
) -> Result<PolicySwitches, ConfigLoaderError> {
    let path = Path::new(governance_root).join("policies.yaml");
    if !path.exists() {
        return Ok(PolicySwitches::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let file: GovernancePoliciesFile = serde_yaml::from_str(&content)?;

    let mut allowed_tags = HashSet::new();
    if let Some(tool_assembly) = &file.tool_assembly {
        if let Some(tags) = &tool_assembly.allowed_tags {
            allowed_tags = tags.iter().cloned().collect();
        }
    }

    Ok(PolicySwitches {
        max_turns: file.max_turns.unwrap_or(16),
        allow_high_risk_tools: file
            .tool_assembly
            .as_ref()
            .map(|t| t.allow_high_risk)
            .unwrap_or(false),
        allowed_capability_tags: allowed_tags,
        denied_tools: file
            .tool_assembly
            .as_ref()
            .map(|t| t.denied_tools.clone())
            .unwrap_or_default(),
        enabled_skills: Vec::new(), // Will be populated from skills.yaml
        policy_version: file.policy_version.unwrap_or_else(|| "1".to_string()),
    })
}

/// Governance policies file structure.
#[derive(Debug, Clone, serde::Deserialize)]
struct GovernancePoliciesFile {
    #[serde(default)]
    policy_version: Option<String>,

    #[serde(default)]
    max_turns: Option<u32>,

    #[serde(default)]
    tool_assembly: Option<GovernanceToolAssembly>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceToolAssembly {
    #[serde(default)]
    #[allow(dead_code)]
    max_tools: Option<usize>,

    #[serde(default)]
    allow_high_risk: bool,

    #[serde(default)]
    allowed_tags: Option<Vec<String>>,

    #[serde(default)]
    denied_tools: HashSet<String>,
}

/// Load subagents from governance/subagents.yaml.
pub fn load_governance_subagents(
    governance_root: &str,
) -> Result<SubagentConfig, ConfigLoaderError> {
    let path = Path::new(governance_root).join("subagents.yaml");
    if !path.exists() {
        return Ok(SubagentConfig::default());
    }

    let content = std::fs::read_to_string(&path)?;
    let file: GovernanceSubagentsFile = serde_yaml::from_str(&content)?;

    Ok(SubagentConfig {
        max_concurrent: file.max_concurrent.unwrap_or(4),
        max_tasks_per_run: file.max_tasks_per_run.unwrap_or(8),
        budget: SubagentBudget::default(),
    })
}

/// Governance subagents file structure.
#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceSubagentsFile {
    #[serde(default)]
    max_concurrent: Option<usize>,

    #[serde(default)]
    max_tasks_per_run: Option<usize>,
}

/// Load enabled skills from governance/skills.yaml.
pub fn load_governance_skills(governance_root: &str) -> Result<Vec<String>, ConfigLoaderError> {
    let path = Path::new(governance_root).join("skills.yaml");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path)?;
    let file: GovernanceSkillsFile = serde_yaml::from_str(&content)?;

    let enabled_skills = file.entries.into_iter().filter(|e| e.enabled).map(|e| e.name).collect();

    Ok(enabled_skills)
}

/// Governance skills file structure.
#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceSkillsFile {
    #[serde(default)]
    entries: Vec<GovernanceSkillEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GovernanceSkillEntry {
    name: String,
    #[serde(default)]
    enabled: bool,
}

/// Merge skills into policy switches.
pub fn merge_skills_into_policies(
    mut policies: PolicySwitches,
    skills: &[String],
) -> PolicySwitches {
    policies.enabled_skills = skills.to_vec();
    policies
}

/// Load ACP agent configurations from governance/acp_agents.yaml.
fn load_governance_acp_agents(
    governance_root: &str,
) -> Result<crate::ACPAgentsConfig, ConfigLoaderError> {
    use std::collections::HashMap;

    let path = Path::new(governance_root).join("acp_agents.yaml");
    if !path.exists() {
        return Ok(crate::ACPAgentsConfig::default());
    }

    let content = std::fs::read_to_string(&path)?;

    // Parse as a map directly
    let agents: HashMap<String, crate::ACPAgentConfig> =
        serde_yaml::from_str(&content).map_err(|e| {
            ConfigLoaderError::InvalidConfig(format!("Failed to parse ACP agents: {}", e))
        })?;

    Ok(crate::ACPAgentsConfig { agents })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_load_governance_models() {
        let dir = tempdir().unwrap();
        let models_path = dir.path().join("models.yaml");
        std::fs::write(
            &models_path,
            r#"
default_model: heuristic
entries:
  - name: heuristic
    provider: builtin
"#,
        )
        .unwrap();

        let result = load_governance_models(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.default_model, Some("heuristic".to_string()));
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "heuristic");
    }

    #[test]
    fn test_load_governance_tools() {
        let dir = tempdir().unwrap();
        let tools_path = dir.path().join("tools.yaml");
        std::fs::write(
            &tools_path,
            r#"
manifests:
  - name: echo
    description: Echo JSON arguments
    capability_tags:
      - builtin
    risk_level: low
    timeout_ms: 30000
    retry_max: 0
    side_effect_class: none
"#,
        )
        .unwrap();

        let result = load_governance_tools(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.manifests.len(), 1);
        assert_eq!(result.manifests[0].name, "echo");
    }

    #[test]
    fn test_load_governance_policies() {
        let dir = tempdir().unwrap();
        let policies_path = dir.path().join("policies.yaml");
        std::fs::write(
            &policies_path,
            r#"
policy_version: "1"
max_turns: 16
tool_assembly:
  max_tools: 32
  allow_high_risk: false
"#,
        )
        .unwrap();

        let result = load_governance_policies(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.max_turns, 16);
        assert!(!result.allow_high_risk_tools);
        assert_eq!(result.policy_version, "1");
    }

    #[test]
    fn test_load_governance_subagents() {
        let dir = tempdir().unwrap();
        let subagents_path = dir.path().join("subagents.yaml");
        std::fs::write(
            &subagents_path,
            r#"
max_concurrent: 4
max_tasks_per_run: 8
"#,
        )
        .unwrap();

        let result = load_governance_subagents(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.max_concurrent, 4);
        assert_eq!(result.max_tasks_per_run, 8);
    }

    #[test]
    fn test_load_governance_acp_agents() {
        let dir = tempdir().unwrap();
        let acp_path = dir.path().join("acp_agents.yaml");
        std::fs::write(
            &acp_path,
            r#"
codex:
  command: codex-acp
  args: ["--model", "gpt-4"]
  description: Codex ACP agent for code generation
  model: gpt-4
  auto_approve_permissions: true
test_agent:
  command: test-agent
  description: Test agent
  auto_approve_permissions: false
"#,
        )
        .unwrap();

        let result = load_governance_acp_agents(dir.path().to_str().unwrap()).unwrap();
        assert!(result.has_agent("codex"));
        assert!(result.has_agent("test_agent"));

        let codex = result.get_agent("codex").unwrap();
        assert_eq!(codex.command, "codex-acp");
        assert_eq!(codex.args, vec!["--model", "gpt-4"]);
        assert!(codex.auto_approve_permissions);
    }
}
