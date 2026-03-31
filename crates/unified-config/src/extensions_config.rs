//! Extensions configuration for MCP servers and skills.
//!
//! This module provides the configuration model for extensions_config.json,
//! which contains MCP server configurations and skill state (enabled/disabled).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
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
