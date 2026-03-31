//! Common types for ecosystem registry

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Registry name
    pub name: String,
    /// Registry URL
    pub url: String,
    /// Whether this is the default registry
    #[serde(default)]
    pub default: bool,
    /// Optional authentication token
    pub token: Option<String>,
}

/// Component types in ecosystem
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComponentType {
    /// MCP server
    McpServer,
    /// Skill
    Skill,
    /// Plugin
    Plugin,
}

/// Metadata for an ecosystem component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentMetadata {
    /// Component ID
    pub id: String,
    /// Component name
    pub name: String,
    /// Component type
    #[serde(rename = "type")]
    pub component_type: ComponentType,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Author(s)
    pub authors: Vec<String>,
    /// Current version
    pub version: String,
    /// All available versions
    pub versions: Vec<String>,
    /// Homepage URL
    pub homepage: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// License
    pub license: Option<String>,
    /// Tags for searching
    pub tags: Vec<String>,
    /// Creation date
    pub created_at: Option<NaiveDateTime>,
    /// Last updated date
    pub updated_at: Option<NaiveDateTime>,
    /// Download count
    pub downloads: u64,
    /// Dependencies on other components
    #[serde(default)]
    pub dependencies: Vec<ComponentDependency>,
    /// Package information
    pub package: PackageInfo,
}

/// Component dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDependency {
    /// Dependency component ID
    pub id: String,
    /// Version constraint
    pub version: String,
    /// Whether this is an optional dependency
    #[serde(default)]
    pub optional: bool,
}

/// Package information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Package type: npm, cargo, pip, git
    pub r#type: String,
    /// Package name in the package manager
    pub name: String,
    /// Installation command or script
    pub install_command: Option<String>,
    /// Environment requirements
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// Search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Total results available
    pub total: usize,
    /// Results for this page
    pub items: Vec<ComponentMetadata>,
    /// Current page
    pub page: usize,
    /// Items per page
    pub per_page: usize,
}

/// Registry error
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("HTTP request error: {0}")]
    HttpRequest(String),

    #[error("JSON deserialization error: {0}")]
    JsonDeserialize(String),

    #[error("Registry API error: {0}")]
    ApiError(String),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("Version not found: {0}")]
    VersionNotFound(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Registry result type
pub type RegistryResult<T> = Result<T, RegistryError>;
