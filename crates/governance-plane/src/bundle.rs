//! Loaded governance bundle (versioned policy + tool manifests).

use std::fs;
use std::path::Path;

use agent_ports::ToolAssemblyPolicy;

use crate::error::{GovernanceError, GovernanceResult};
use crate::models::ModelsFile;
use crate::policies::PoliciesFile;
use crate::subagents::SubagentsFile;
use crate::tools::ToolsFile;

#[derive(Debug, Clone)]
pub struct GovernanceBundle {
    pub policy_version: String,
    pub models: ModelsFile,
    pub tools: ToolsFile,
    pub policies: PoliciesFile,
    pub subagents: SubagentsFile,
}

impl GovernanceBundle {
    /// Load YAML files from a directory (`models.yaml`, `tools.yaml`, `policies.yaml`, `subagents.yaml`).
    pub fn load_from_dir(dir: impl AsRef<Path>) -> GovernanceResult<Self> {
        let dir = dir.as_ref();
        let models = read_yaml::<ModelsFile>(&dir.join("models.yaml")).unwrap_or_default();
        let tools = read_yaml::<ToolsFile>(&dir.join("tools.yaml")).unwrap_or_default();
        let policies = read_yaml::<PoliciesFile>(&dir.join("policies.yaml")).unwrap_or_default();
        let subagents = read_yaml::<SubagentsFile>(&dir.join("subagents.yaml")).unwrap_or_default();
        let policy_version = policies.policy_version.clone();
        Ok(Self { policy_version, models, tools, policies, subagents })
    }

    #[must_use]
    pub fn tool_assembly(&self) -> ToolAssemblyPolicy {
        self.policies.tool_assembly.clone()
    }
}

fn read_yaml<T: serde::de::DeserializeOwned + Default>(path: &Path) -> GovernanceResult<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = fs::read_to_string(path)?;
    serde_yaml::from_str(&raw).map_err(|e| GovernanceError::Yaml(e.to_string()))
}

impl Default for GovernanceBundle {
    fn default() -> Self {
        Self {
            policy_version: "0".into(),
            models: ModelsFile::default(),
            tools: ToolsFile::default(),
            policies: PoliciesFile::default(),
            subagents: SubagentsFile::default(),
        }
    }
}
