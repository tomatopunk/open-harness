//! Unified configuration model for open-harness.
//!
//! This crate provides a single source of truth for:
//! - Model configurations (LLM providers, API keys, parameters)
//! - Tool manifests and assembly policies
//! - Subagent execution constraints
//! - Policy switches and governance rules

pub mod config_watcher;
pub mod extensions_config;
pub mod loader;
pub mod migrate;

pub use config_watcher::{ConfigManager, ConfigWatcher};
pub use extensions_config::{ExtensionsConfig, McpServerConfig, SkillState};

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Unified configuration - the single source of truth for all runtime components.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedConfig {
    /// Model registry with complete configurations
    #[serde(default)]
    pub models: ModelRegistry,

    /// Tool manifests and assembly policies
    #[serde(default)]
    pub tools: ToolRegistry,

    /// Subagent execution configuration
    #[serde(default)]
    pub subagents: SubagentConfig,

    /// Policy switches and governance rules
    #[serde(default)]
    pub policies: PolicySwitches,

    /// ACP agent configurations
    #[serde(default)]
    pub acp_agents: ACPAgentsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelRuntimeConfigView {
    pub llm: KernelRuntimeLlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelRuntimeLlmConfig {
    pub name: String,

    pub provider: String,

    pub model_id: String,

    #[serde(default)]
    pub config: ModelConfig,
}

impl UnifiedConfig {
    pub fn kernel_runtime_view(
        &self,
    ) -> Result<KernelRuntimeConfigView, loader::ConfigLoaderError> {
        let selected_model = self.models.selected_kernel_model()?;

        Ok(KernelRuntimeConfigView {
            llm: KernelRuntimeLlmConfig {
                name: selected_model.name.clone(),
                provider: selected_model.provider.clone(),
                model_id: selected_model.model_id.clone(),
                config: selected_model.config.clone(),
            },
        })
    }
}

/// Model registry containing all available LLM models.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRegistry {
    /// Default model name to use when not specified
    #[serde(default)]
    pub default_model: String,

    /// All registered model entries
    #[serde(default)]
    pub entries: Vec<ModelEntry>,
}

impl ModelRegistry {
    pub fn selected_kernel_model(&self) -> Result<&ModelEntry, loader::ConfigLoaderError> {
        if self.entries.is_empty() {
            return Err(loader::ConfigLoaderError::validation(
                "unified config",
                None,
                "At least one model must be configured",
            ));
        }

        if self.default_model.is_empty() {
            return Ok(&self.entries[0]);
        }

        self.entries.iter().find(|model| model.name == self.default_model).ok_or_else(|| {
            loader::ConfigLoaderError::validation(
                "unified config",
                None,
                format!("default_model '{}' must match a configured model", self.default_model),
            )
        })
    }
}

/// A single model entry with complete configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Unique identifier for this model
    pub name: String,

    /// Human-readable display name
    pub display_name: String,

    /// Provider type (e.g., "langchain_openai:ChatOpenAI")
    pub provider: String,

    /// Model ID used by the provider (e.g., "gpt-4")
    pub model_id: String,

    /// Complete model configuration
    #[serde(default)]
    pub config: ModelConfig,
}

/// Complete model configuration including API keys and parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelConfig {
    /// API key or environment variable reference (e.g., "$OPENAI_API_KEY")
    #[serde(default)]
    pub api_key: Option<String>,

    /// Maximum tokens to generate
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Sampling temperature
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Base URL for API endpoint
    #[serde(default)]
    pub base_url: Option<String>,

    /// Whether to use Responses API (OpenAI-specific)
    #[serde(default)]
    pub use_responses_api: Option<bool>,

    /// Output version for Responses API
    #[serde(default)]
    pub output_version: Option<String>,

    /// Additional provider-specific configuration
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Tool registry containing all available tools and assembly policies.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolRegistry {
    /// All tool manifests
    #[serde(default)]
    pub manifests: Vec<ToolManifest>,

    /// Tool assembly policy
    #[serde(default)]
    pub assembly_policy: ToolAssemblyPolicy,
}

/// Tool manifest describing capabilities and constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    /// Unique tool name
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// Capability tags (e.g., "builtin", "network", "filesystem")
    #[serde(default)]
    pub capability_tags: Vec<String>,

    /// Risk level classification
    #[serde(default)]
    pub risk_level: RiskLevel,

    /// Timeout in milliseconds
    #[serde(default)]
    pub timeout_ms: u64,

    /// Maximum retry attempts
    #[serde(default)]
    pub retry_max: u32,

    /// Side effect classification
    #[serde(default)]
    pub side_effect_class: SideEffectClass,

    /// Provider type (local, mcp, skill, community)
    #[serde(default)]
    pub provider_type: ToolProviderType,

    /// Provider name
    #[serde(default)]
    pub provider_name: String,

    /// Load path for dynamic tools
    #[serde(default)]
    pub load_path: Option<String>,

    /// Tool version
    #[serde(default)]
    pub version: Option<String>,

    /// JSON Schema for input validation
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
}

/// Risk level classification for tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
}

/// Side effect classification for tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SideEffectClass {
    #[default]
    None,
    Read,
    Write,
    Network,
    Exec,
}

/// Tool provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolProviderType {
    #[default]
    Local,
    Mcp,
    Skill,
    Community,
}

/// Tool assembly policy controlling which tools can be loaded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAssemblyPolicy {
    /// Allowed capability tags
    #[serde(default)]
    pub allowed_tags: HashSet<String>,

    /// Denied tool names
    #[serde(default)]
    pub denied_tools: HashSet<String>,

    /// Maximum number of tools allowed
    #[serde(default = "default_max_tools")]
    pub max_tools: usize,

    /// Whether to allow high-risk tools
    #[serde(default)]
    pub allow_high_risk: bool,
}

fn default_max_tools() -> usize {
    32
}

/// Subagent execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    /// Maximum concurrent subagent tasks
    #[serde(default = "default_max_concurrent_subagents")]
    pub max_concurrent: usize,

    /// Maximum tasks per run
    #[serde(default = "default_max_tasks_per_run")]
    pub max_tasks_per_run: usize,

    /// Budget configuration for subagents
    #[serde(default)]
    pub budget: SubagentBudget,
}

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent_subagents(),
            max_tasks_per_run: default_max_tasks_per_run(),
            budget: SubagentBudget::default(),
        }
    }
}

fn default_max_concurrent_subagents() -> usize {
    4
}

fn default_max_tasks_per_run() -> usize {
    8
}

/// Budget configuration for subagent execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubagentBudget {
    /// Maximum turns allowed
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,

    /// Maximum subagent tasks
    #[serde(default)]
    pub max_subagent_tasks: u32,

    /// Task cap per response
    #[serde(default)]
    pub subagent_task_cap_per_response: u32,

    /// Maximum concurrent tool calls
    #[serde(default)]
    pub max_concurrent_tool_calls: u32,

    /// Per-task timeout in milliseconds
    #[serde(default)]
    pub per_task_timeout_ms: Option<u64>,

    /// Maximum total wall time in milliseconds
    #[serde(default)]
    pub max_total_wall_time_ms: Option<u64>,

    /// Token budget
    #[serde(default)]
    pub token_budget: Option<u64>,
}

fn default_max_turns() -> u32 {
    16
}

/// Policy switches and governance rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySwitches {
    /// Maximum turns per agent run
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,

    /// Whether to allow high-risk tools
    #[serde(default)]
    pub allow_high_risk_tools: bool,

    /// Allowed capability tags
    #[serde(default)]
    pub allowed_capability_tags: HashSet<String>,

    /// Denied tool names
    #[serde(default)]
    pub denied_tools: HashSet<String>,

    /// Enabled skill names
    #[serde(default)]
    pub enabled_skills: Vec<String>,

    /// Policy version for tracking
    #[serde(default)]
    pub policy_version: String,
}

impl Default for PolicySwitches {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            allow_high_risk_tools: false,
            allowed_capability_tags: HashSet::new(),
            denied_tools: HashSet::new(),
            enabled_skills: Vec::new(),
            policy_version: "1".to_string(),
        }
    }
}

/// ACP (Agent Client Protocol) agents configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ACPAgentsConfig {
    /// Map of agent name to configuration
    #[serde(default, flatten)]
    pub agents: std::collections::HashMap<String, ACPAgentConfig>,
}

impl ACPAgentsConfig {
    /// Get a specific agent configuration by name
    pub fn get_agent(&self, name: &str) -> Option<&ACPAgentConfig> {
        self.agents.get(name)
    }

    /// Check if an agent exists
    pub fn has_agent(&self, name: &str) -> bool {
        self.agents.contains_key(name)
    }

    /// Get all agent names
    pub fn agent_names(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }
}

/// Configuration for a single ACP-compatible agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ACPAgentConfig {
    /// Command to launch the ACP agent subprocess
    pub command: String,

    /// Additional command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// Description of the agent's capabilities (shown in tool description)
    pub description: String,

    /// Model hint passed to the agent (optional)
    #[serde(default)]
    pub model: Option<String>,

    /// When true, automatically approve all ACP permission requests
    /// (allow_once preferred over allow_always). When false (default),
    /// all permission requests are denied.
    #[serde(default)]
    pub auto_approve_permissions: bool,

    /// Timeout in milliseconds for agent execution (default: 300000ms = 5min)
    #[serde(default = "default_acp_timeout")]
    pub timeout_ms: u64,
}

fn default_acp_timeout() -> u64 {
    300_000 // 5 minutes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_config_default() {
        let config = UnifiedConfig::default();
        assert_eq!(config.policies.max_turns, 16);
        assert_eq!(config.subagents.max_concurrent, 4);
        assert_eq!(config.subagents.max_tasks_per_run, 8);
    }

    #[test]
    fn test_model_entry_serialization() {
        let entry = ModelEntry {
            name: "gpt-4".to_string(),
            display_name: "GPT-4".to_string(),
            provider: "langchain_openai:ChatOpenAI".to_string(),
            model_id: "gpt-4".to_string(),
            config: ModelConfig {
                api_key: Some("$OPENAI_API_KEY".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                ..Default::default()
            },
        };

        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: ModelEntry = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.name, "gpt-4");
        assert_eq!(deserialized.config.max_tokens, Some(4096));
    }

    #[test]
    fn test_tool_manifest_serialization() {
        let manifest = ToolManifest {
            name: "echo".to_string(),
            description: Some("Echo JSON arguments".to_string()),
            capability_tags: vec!["builtin".to_string()],
            risk_level: RiskLevel::Low,
            timeout_ms: 30000,
            retry_max: 0,
            side_effect_class: SideEffectClass::None,
            provider_type: ToolProviderType::Local,
            provider_name: "builtin".to_string(),
            load_path: None,
            version: Some("1.0.0".to_string()),
            input_schema: None,
        };

        let serialized = serde_json::to_string(&manifest).unwrap();
        let deserialized: ToolManifest = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.name, "echo");
        assert_eq!(deserialized.risk_level, RiskLevel::Low);
    }

    #[test]
    fn test_acp_agent_config() {
        let config = ACPAgentConfig {
            command: "codex-acp".to_string(),
            args: vec!["--model".to_string(), "gpt-4".to_string()],
            description: "Codex ACP agent".to_string(),
            model: Some("gpt-4".to_string()),
            auto_approve_permissions: true,
            timeout_ms: 300_000,
        };

        assert_eq!(config.command, "codex-acp");
        assert!(config.auto_approve_permissions);
        assert_eq!(config.model, Some("gpt-4".to_string()));
        assert_eq!(config.timeout_ms, 300_000);
    }

    #[test]
    fn test_kernel_runtime_view_uses_explicit_default_model() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: "gpt-4o".to_string(),
                entries: vec![
                    ModelEntry {
                        name: "gpt-4".to_string(),
                        display_name: "GPT-4".to_string(),
                        provider: "langchain_openai:ChatOpenAI".to_string(),
                        model_id: "gpt-4".to_string(),
                        config: ModelConfig::default(),
                    },
                    ModelEntry {
                        name: "gpt-4o".to_string(),
                        display_name: "GPT-4o".to_string(),
                        provider: "open_ai".to_string(),
                        model_id: "gpt-4o".to_string(),
                        config: ModelConfig {
                            api_key: Some("$OPENAI_API_KEY".to_string()),
                            max_tokens: Some(4096),
                            temperature: Some(0.2),
                            ..Default::default()
                        },
                    },
                ],
            },
            ..Default::default()
        };

        let runtime_view = config.kernel_runtime_view().unwrap();

        assert_eq!(runtime_view.llm.name, "gpt-4o");
        assert_eq!(runtime_view.llm.provider, "open_ai");
        assert_eq!(runtime_view.llm.model_id, "gpt-4o");
        assert_eq!(runtime_view.llm.config.max_tokens, Some(4096));
        assert_eq!(runtime_view.llm.config.temperature, Some(0.2));
    }

    #[test]
    fn test_kernel_runtime_view_falls_back_to_first_model_when_default_missing() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: String::new(),
                entries: vec![ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "langchain_openai:ChatOpenAI".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let runtime_view = config.kernel_runtime_view().unwrap();

        assert_eq!(runtime_view.llm.name, "gpt-4");
        assert_eq!(runtime_view.llm.model_id, "gpt-4");
    }

    #[test]
    fn test_kernel_runtime_view_rejects_unknown_default_model() {
        let config = UnifiedConfig {
            models: ModelRegistry {
                default_model: "missing".to_string(),
                entries: vec![ModelEntry {
                    name: "gpt-4".to_string(),
                    display_name: "GPT-4".to_string(),
                    provider: "langchain_openai:ChatOpenAI".to_string(),
                    model_id: "gpt-4".to_string(),
                    config: ModelConfig::default(),
                }],
            },
            ..Default::default()
        };

        let error = config.kernel_runtime_view().unwrap_err();

        assert!(error.to_string().contains("default_model 'missing'"));
    }
}
