//! Sandbox execution environment for packages

use crate::types::PackageManagerResult;
use std::path::PathBuf;
use tracing::debug;

/// Sandbox configuration
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Root directory for all sandboxes
    pub root: PathBuf,
    /// Whether to enable network access
    pub network_enabled: bool,
    /// Memory limit in MB
    pub memory_limit_mb: Option<u64>,
    /// CPU limit in percentage
    pub cpu_limit_percent: Option<u32>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("./.sandbox"),
            network_enabled: true,
            memory_limit_mb: None,
            cpu_limit_percent: None,
        }
    }
}

/// Sandbox for package execution
pub struct PackageSandbox {
    config: SandboxConfig,
}

impl PackageSandbox {
    /// Create new sandbox
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    /// Initialize sandbox
    pub async fn initialize(&self) -> PackageManagerResult<()> {
        if !self.config.root.exists() {
            tokio::fs::create_dir_all(&self.config.root).await.map_err(|e| {
                crate::types::PackageManagerError::Io(format!(
                    "Failed to create sandbox root: {}",
                    e
                ))
            })?;
        }
        Ok(())
    }

    /// Create a sandbox for a specific package
    pub async fn create_sandbox(&self, package_name: &str) -> PackageManagerResult<PathBuf> {
        let sandbox_path = self.config.root.join(sanitize_filename(package_name));
        if !sandbox_path.exists() {
            tokio::fs::create_dir_all(&sandbox_path).await.map_err(|e| {
                crate::types::PackageManagerError::Io(format!(
                    "Failed to create sandbox directory: {}",
                    e
                ))
            })?;
        }
        debug!("Created sandbox for {} at {:?}", package_name, sandbox_path);
        Ok(sandbox_path)
    }

    /// Delete a sandbox
    pub async fn delete_sandbox(&self, package_name: &str) -> PackageManagerResult<()> {
        let sandbox_path = self.config.root.join(sanitize_filename(package_name));
        if sandbox_path.exists() {
            tokio::fs::remove_dir_all(&sandbox_path).await.map_err(|e| {
                crate::types::PackageManagerError::Io(format!("Failed to delete sandbox: {}", e))
            })?;
            debug!("Deleted sandbox for {}", package_name);
        }
        Ok(())
    }

    /// Get sandbox path for a package
    pub fn get_sandbox_path(&self, package_name: &str) -> PathBuf {
        self.config.root.join(sanitize_filename(package_name))
    }
}

/// Sanitize filename for filesystem
fn sanitize_filename(name: &str) -> String {
    name.replace(|c: char| !c.is_alphanumeric() && c != '-', "_")
}
