use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubagentsFile {
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub max_tasks_per_run: u32,
}
