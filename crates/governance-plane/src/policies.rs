use serde::{Deserialize, Serialize};

use agent_ports::ToolAssemblyPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoliciesFile {
    #[serde(default)]
    pub policy_version: String,
    #[serde(default)]
    pub tool_assembly: ToolAssemblyPolicy,
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
}

fn default_max_turns() -> u32 {
    16
}

impl Default for PoliciesFile {
    fn default() -> Self {
        Self {
            policy_version: String::new(),
            tool_assembly: ToolAssemblyPolicy::default(),
            max_turns: default_max_turns(),
        }
    }
}
