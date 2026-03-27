use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsRuntime {
    pub enabled_skills: Vec<String>,
}

impl SkillsRuntime {
    pub fn with_default_skill() -> Self {
        Self { enabled_skills: vec!["research".to_string()] }
    }
}
