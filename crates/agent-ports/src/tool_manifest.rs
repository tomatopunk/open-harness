//! Configuration-driven tool manifests and assembly policy.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Declarative tool metadata (schema + policy hooks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for arguments (opaque to core; validated by registry).
    #[serde(default)]
    pub input_schema: Option<Value>,
    #[serde(default)]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retry_max: u32,
    #[serde(default)]
    pub side_effect_class: SideEffectClass,
    /// Tool provider type (local, mcp, skill, community)
    #[serde(default)]
    pub provider_type: ToolProviderType,
    /// Tool provider name
    #[serde(default)]
    pub provider_name: String,
    /// Dynamic load path (for lazily loaded tools)
    #[serde(default)]
    pub load_path: Option<String>,
    /// Tool version
    #[serde(default)]
    pub version: Option<String>,
}

/// Tool provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderType {
    #[default]
    Local,
    Mcp,
    Skill,
    Community,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClass {
    #[default]
    None,
    Read,
    Write,
    Network,
    Exec,
}

/// Filters manifests into the active toolset for a turn.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolAssemblyPolicy {
    #[serde(default)]
    pub allowed_tags: HashSet<String>,
    #[serde(default)]
    pub denied_tools: HashSet<String>,
    #[serde(default)]
    pub max_tools: usize,
    #[serde(default)]
    pub allow_high_risk: bool,
}

impl ToolAssemblyPolicy {
    #[must_use]
    pub fn resolve<'a>(&self, manifests: &'a [ToolManifest]) -> Vec<&'a ToolManifest> {
        let mut out: Vec<&ToolManifest> = manifests
            .iter()
            .filter(|m| !self.denied_tools.contains(&m.name))
            .filter(|m| {
                if m.risk_level == RiskLevel::High && !self.allow_high_risk {
                    return false;
                }
                true
            })
            .filter(|m| {
                if self.allowed_tags.is_empty() {
                    return true;
                }
                m.capability_tags.iter().any(|t| self.allowed_tags.contains(t))
            })
            .collect();
        if self.max_tools > 0 && out.len() > self.max_tools {
            out.truncate(self.max_tools);
        }
        out
    }
}
