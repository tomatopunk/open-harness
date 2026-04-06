//! Extensions configuration for MCP servers and skills.
//!
//! This module provides the configuration model for extensions_config.json,
//! which contains MCP server configurations and skill state (enabled/disabled).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Extensions configuration containing MCP servers and skills.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionsConfig {
    /// MCP server configurations
    #[serde(alias = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// Skill states (enabled/disabled with version)
    pub skills: HashMap<String, SkillState>,
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Whether this MCP server is enabled
    pub enabled: bool,
    /// Transport type: "stdio", "sse", or "http"
    pub r#type: String,
    /// Command to execute (for stdio type)
    pub command: Option<String>,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Server URL (for sse or http type)
    pub url: Option<String>,
    /// OAuth configuration
    pub oauth: Option<McpOAuthConfig>,
    /// Human-readable description
    #[serde(default)]
    pub description: String,
}

/// OAuth configuration for MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    /// Whether OAuth is enabled
    pub enabled: bool,
    /// Token endpoint URL
    pub token_url: String,
    /// Grant type
    pub grant_type: String,
    /// Client ID
    pub client_id: Option<String>,
    /// Client secret
    pub client_secret: Option<String>,
    /// Refresh token
    pub refresh_token: Option<String>,
    /// Scope
    pub scope: Option<String>,
}

/// State for a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    /// Whether the skill is enabled
    pub enabled: bool,
    /// Current version
    pub version: String,
}

impl ExtensionsConfig {
    /// Load from JSON file.
    pub fn from_file(path: &Path) -> Result<Self, crate::loader::ConfigLoaderError> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            crate::loader::ConfigLoaderError::io("extensions config", path, error)
        })?;
        let config: Self = serde_json::from_str(&content).map_err(|error| {
            crate::loader::ConfigLoaderError::parse("extensions config", path, "json", error)
        })?;
        config.validate(path)?;
        Ok(config)
    }

    fn validate(&self, path: &Path) -> Result<(), crate::loader::ConfigLoaderError> {
        for (name, server) in &self.mcp_servers {
            match server.r#type.as_str() {
                "stdio" | "sse" | "http" => {}
                other => {
                    return Err(crate::loader::ConfigLoaderError::validation(
                        "extensions config",
                        Some(path),
                        format!("MCP server '{name}' has unsupported transport type '{other}'"),
                    ));
                }
            }
        }

        for (name, state) in &self.skills {
            if state.version.trim().is_empty() {
                return Err(crate::loader::ConfigLoaderError::validation(
                    "extensions config",
                    Some(path),
                    format!("Skill '{name}' must declare a non-empty version"),
                ));
            }
        }

        Ok(())
    }

    /// Get enabled MCP servers.
    pub fn enabled_mcp_servers(&self) -> Vec<(&String, &McpServerConfig)> {
        self.mcp_servers.iter().filter(|(_, config)| config.enabled).collect()
    }

    /// Get enabled skills.
    pub fn enabled_skills(&self) -> Vec<(&String, &SkillState)> {
        self.skills.iter().filter(|(_, state)| state.enabled).collect()
    }
}
