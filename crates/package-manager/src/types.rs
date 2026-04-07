//! Common types for package manager

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Package manager configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageManagerConfig {
    /// Configuration for npm
    #[serde(default)]
    pub npm: NpmConfig,
    /// Configuration for cargo
    #[serde(default)]
    pub cargo: CargoConfig,
    /// Configuration for pip
    #[serde(default)]
    pub pip: PipConfig,
    /// Installation root directory
    pub install_root: PathBuf,
    /// Whether to enable sandboxing
    #[serde(default)]
    pub sandbox_enabled: bool,
    /// Auto-check for updates interval in seconds
    #[serde(default)]
    pub auto_update_interval: Option<u64>,
}

/// npm configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmConfig {
    /// Whether npm is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Custom registry URL
    pub registry: Option<String>,
    /// npm executable path
    pub executable: Option<String>,
}

impl Default for NpmConfig {
    fn default() -> Self {
        Self { enabled: true, registry: None, executable: Some("npm".to_string()) }
    }
}

/// cargo configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoConfig {
    /// Whether cargo is enabled
    #[serde(default)]
    pub enabled: bool,
    /// cargo executable path
    pub executable: Option<String>,
    /// cargo registry
    pub registry: Option<String>,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self { enabled: true, executable: Some("cargo".to_string()), registry: None }
    }
}

/// pip configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipConfig {
    /// Whether pip is enabled
    #[serde(default)]
    pub enabled: bool,
    /// pip executable path
    pub executable: Option<String>,
    /// PyPI index URL
    pub index_url: Option<String>,
}

impl Default for PipConfig {
    fn default() -> Self {
        Self { enabled: true, executable: Some("pip".to_string()), index_url: None }
    }
}

/// Package types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PackageType {
    /// npm package
    Npm,
    /// cargo crate
    Cargo,
    /// PyPI package
    Pip,
    /// Git repository
    Git,
}

/// Install request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRequest {
    /// Package name
    pub name: String,
    /// Package type
    pub package_type: PackageType,
    /// Version specification (semver or git ref)
    pub version: Option<String>,
    /// Git repository URL (for Git type)
    pub git_url: Option<String>,
    /// Installation directory (overrides default)
    pub install_dir: Option<PathBuf>,
    /// Force reinstall even if already installed
    #[serde(default)]
    pub force: bool,
}

/// Install result
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Whether installation succeeded
    pub success: bool,
    /// Installed package path
    pub installed_path: Option<PathBuf>,
    /// Installed version
    pub installed_version: Option<String>,
    /// Any warnings
    pub warnings: Vec<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Package info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Package name
    pub name: String,
    /// Package type
    pub package_type: PackageType,
    /// Installed version
    pub version: String,
    /// Installation path
    pub install_path: PathBuf,
    /// Installation timestamp
    pub installed_at: u64,
    /// Last update timestamp
    pub updated_at: u64,
    /// Dependencies
    pub dependencies: Vec<String>,
}

/// Package manager error
#[derive(Debug, Error)]
pub enum PackageManagerError {
    #[error("Package manager not available: {0}")]
    NotAvailable(String),

    #[error("Package already installed: {0}")]
    AlreadyInstalled(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Version not satisfied: {0}")]
    VersionNotSatisfied(String),

    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Dependency conflict: {0}")]
    DependencyConflict(String),

    #[error("Sandbox error: {0}")]
    SandboxError(String),

    #[error("Unsupported package type: {0:?}")]
    UnsupportedType(PackageType),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Package manager result
pub type PackageManagerResult<T> = Result<T, PackageManagerError>;
