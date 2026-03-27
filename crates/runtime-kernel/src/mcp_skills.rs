use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerConfig {
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: Option<String>,
    #[serde(default)]
    pub oauth: Option<McpOAuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsRuntime {
    pub enabled_skills: Vec<String>,
    #[serde(default)]
    pub install_root: Option<String>,
}

impl SkillsRuntime {
    pub fn with_default_skill() -> Self {
        Self { enabled_skills: vec!["research".to_string()], install_root: None }
    }

    pub fn validate_skill_archive(filename: &str) -> Result<(), String> {
        if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
            return Err("invalid archive name".to_string());
        }
        if !filename.ends_with(".skill") {
            return Err("skill archive must end with .skill".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpOAuthConfig {
    pub client_id: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl McpServerConfig {
    pub fn oauth_enabled(&self) -> bool {
        self.oauth.as_ref().map(|oauth| !oauth.client_id.is_empty()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_archive_name_is_validated() {
        assert!(SkillsRuntime::validate_skill_archive("pack.skill").is_ok());
        assert!(SkillsRuntime::validate_skill_archive("../pack.skill").is_err());
        assert!(SkillsRuntime::validate_skill_archive("pack.zip").is_err());
    }
}
