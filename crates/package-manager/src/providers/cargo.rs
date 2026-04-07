//! cargo package provider

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

use crate::traits::PackageProvider;
use crate::types::{
    CargoConfig, InstallRequest, InstallResult, InstalledPackage, PackageManagerError,
    PackageManagerResult, PackageType,
};

/// Cargo package provider
pub struct CargoProvider {
    config: CargoConfig,
    install_root: PathBuf,
}

impl CargoProvider {
    /// Create new cargo provider
    pub fn new(config: CargoConfig, install_root: PathBuf) -> Self {
        Self { config, install_root }
    }

    /// Get cargo executable path
    fn cargo_path(&self) -> &str {
        self.config.executable.as_deref().unwrap_or("cargo")
    }
}

#[async_trait]
impl PackageProvider for CargoProvider {
    fn name(&self) -> &str {
        "cargo"
    }

    async fn check_availability(&self) -> PackageManagerResult<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        let output = Command::new(self.cargo_path()).arg("--version").output().await;

        Ok(output.is_ok())
    }

    async fn install(&self, request: &InstallRequest) -> PackageManagerResult<InstallResult> {
        let install_dir = request
            .install_dir
            .clone()
            .unwrap_or_else(|| self.install_root.join("cargo").join(&request.name));

        // Create directory if it doesn't exist
        if !install_dir.exists() {
            tokio::fs::create_dir_all(&install_dir)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to create dir: {}", e)))?;
        }

        // Create Cargo.toml
        let cargo_toml = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
{} = "{}"
"#,
            request.name,
            request.name,
            request.version.as_deref().unwrap_or("*")
        );

        let cargo_toml_path = install_dir.join("Cargo.toml");
        tokio::fs::write(&cargo_toml_path, cargo_toml)
            .await
            .map_err(|e| PackageManagerError::Io(format!("Failed to write Cargo.toml: {}", e)))?;

        let mut command = Command::new(self.cargo_path());
        command
            .arg("build")
            .current_dir(&install_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(registry) = &self.config.registry {
            command.arg("--registry").arg(registry);
        }

        tracing::debug!("Running cargo build: {:?}", command);

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!("Failed to run cargo build: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                installed_path: None,
                installed_version: None,
                warnings: Vec::new(),
                error: Some(stderr.to_string()),
            });
        }

        // Try to get version from cargo.lock
        let installed_version = request.version.clone();

        Ok(InstallResult {
            success: true,
            installed_path: Some(install_dir),
            installed_version,
            warnings: Vec::new(),
            error: None,
        })
    }

    async fn uninstall(&self, name: &str) -> PackageManagerResult<()> {
        let install_path = self.install_root.join("cargo").join(name);
        if install_path.exists() {
            tokio::fs::remove_dir_all(&install_path)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to uninstall: {}", e)))?;
        }
        Ok(())
    }

    async fn update(&self, name: &str) -> PackageManagerResult<InstallResult> {
        let install_dir = self.install_root.join("cargo").join(name);
        if !install_dir.exists() {
            return Err(PackageManagerError::PackageNotFound(name.to_string()));
        }

        let mut command = Command::new(self.cargo_path());
        command
            .arg("update")
            .current_dir(&install_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!("Failed to run cargo update: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(InstallResult {
                success: false,
                installed_path: Some(install_dir),
                installed_version: None,
                warnings: Vec::new(),
                error: Some(stderr.to_string()),
            });
        }

        Ok(InstallResult {
            success: true,
            installed_path: Some(install_dir),
            installed_version: None,
            warnings: Vec::new(),
            error: None,
        })
    }

    async fn list_installed(&self) -> PackageManagerResult<Vec<InstalledPackage>> {
        let cargo_root = self.install_root.join("cargo");
        if !cargo_root.exists() {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();
        let mut entries = tokio::fs::read_dir(&cargo_root)
            .await
            .map_err(|e| PackageManagerError::Io(format!("Failed to read dir: {}", e)))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| PackageManagerError::Io(format!("Failed to read entry: {}", e)))?
        {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or_default() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(Some(pkg)) = self.get_installed(&name).await {
                    packages.push(pkg);
                }
            }
        }

        Ok(packages)
    }

    async fn get_installed(&self, name: &str) -> PackageManagerResult<Option<InstalledPackage>> {
        let install_path = self.install_root.join("cargo").join(name);
        if !install_path.exists() {
            return Ok(None);
        }

        let cargo_toml_path = install_path.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            return Ok(None);
        }

        let installed_at = std::fs::metadata(&cargo_toml_path)
            .ok()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Parse version from Cargo.toml - this is simplified
        let content = tokio::fs::read_to_string(&cargo_toml_path)
            .await
            .map_err(|e| PackageManagerError::Io(format!("Failed to read Cargo.toml: {}", e)))?;
        let version = content
            .lines()
            .find(|line| line.starts_with("version ="))
            .and_then(|line| line.split('=').nth(1).map(|v| v.trim().trim_matches('"').to_string()))
            .unwrap_or_else(|| "unknown".to_string());

        Ok(Some(InstalledPackage {
            name: name.to_string(),
            package_type: PackageType::Cargo,
            version,
            install_path,
            installed_at,
            updated_at: installed_at,
            dependencies: Vec::new(),
        }))
    }

    async fn check_updates(&self) -> PackageManagerResult<Vec<(String, String, String)>> {
        // For cargo, this would require more complex parsing of Cargo.lock
        // For simplicity, we return empty for now
        Ok(Vec::new())
    }
}
