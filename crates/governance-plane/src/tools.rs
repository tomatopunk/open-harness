use serde::{Deserialize, Serialize};

use agent_ports::ToolManifest;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsFile {
    #[serde(default)]
    pub manifests: Vec<ToolManifest>,
}
