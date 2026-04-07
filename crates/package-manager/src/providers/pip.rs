//! pip package provider

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

use crate::traits::PackageProvider;
use crate::types::{
    InstallRequest, InstallResult, InstalledPackage, PackageManagerError, PackageManagerResult,
    PackageType, PipConfig,
};

/// pip package provider
pub struct PipProvider {
    config: PipConfig,
    install_root: PathBuf,
}

impl PipProvider {
    /// Create new pip provider
    pub fn new(config: PipConfig, install_root: PathBuf) -> Self {
        Self { config, install_root }
    }

    /// Get pip executable path
    fn pip_path(&self) -> &str {
        self.config.executable.as_deref().unwrap_or("pip")
    }
}

#[async_trait]
impl PackageProvider for PipProvider {
    fn name(&self) -> &str {
        "pip"
    }

    async fn check_availability(&self) -> PackageManagerResult<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        let output = Command::new(self.pip_path()).arg("--version").output().await;

        Ok(output.is_ok())
    }

    async fn install(&self, request: &InstallRequest) -> PackageManagerResult<InstallResult> {
        let package_spec = if let Some(version) = &request.version {
            format!("{}=={}", request.name, version)
        } else {
            request.name.clone()
        };

        let install_dir = request
            .install_dir
            .clone()
            .unwrap_or_else(|| self.install_root.join("pip").join(&request.name));

        // Create directory if it doesn't exist
        if !install_dir.exists() {
            tokio::fs::create_dir_all(&install_dir)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to create dir: {}", e)))?;
        }

        let mut command = Command::new(self.pip_path());
        command
            .arg("install")
            .arg("--target")
            .arg(&install_dir)
            .arg(&package_spec)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(index_url) = &self.config.index_url {
            command.arg("--index-url").arg(index_url);
        }

        tracing::debug!("Running pip install: {:?}", command);

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!("Failed to run pip install: {}", e))
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

        // Try to get installed version
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
        let install_path = self.install_root.join("pip").join(name);
        if install_path.exists() {
            tokio::fs::remove_dir_all(&install_path)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to uninstall: {}", e)))?;
        }
        Ok(())
    }

    async fn update(&self, name: &str) -> PackageManagerResult<InstallResult> {
        let install_dir = self.install_root.join("pip").join(name);
        if !install_dir.exists() {
            return Err(PackageManagerError::PackageNotFound(name.to_string()));
        }

        let mut command = Command::new(self.pip_path());
        command
            .arg("install")
            .arg("--upgrade")
            .arg(name)
            .arg("--target")
            .arg(&install_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!(
                "Failed to run pip install --upgrade: {}",
                e
            ))
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
        let pip_root = self.install_root.join("pip");
        if !pip_root.exists() {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();
        let mut entries = tokio::fs::read_dir(&pip_root)
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
        let install_path = self.install_root.join("pip").join(name);
        if !install_path.exists() {
            return Ok(None);
        }

        let installed_at = std::fs::metadata(&install_path)
            .ok()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Try to find package info
        let mut installed_version = None;
        let mut dir_entries = tokio::fs::read_dir(&install_path).await.map_err(|e| {
            PackageManagerError::Io(format!("Failed to read install directory: {}", e))
        })?;
        'outer: loop {
            let entry = match dir_entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.contains(name) {
                if let Ok(metadata) = entry.metadata().await {
                    if metadata.is_dir() {
                        let dist_info =
                            entry.path().join(format!("{}-{}.dist-info", name, "version"));
                        if dist_info.exists() {
                            // This is simplified - actual implementation would parse METADATA
                            if let Some((_, version)) = file_name.rsplit_once('-') {
                                if version.ends_with(".dist-info") {
                                    installed_version =
                                        Some(version.trim_end_matches(".dist-info").to_string());
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Some(InstalledPackage {
            name: name.to_string(),
            package_type: PackageType::Pip,
            version: installed_version.unwrap_or_else(|| "unknown".to_string()),
            install_path,
            installed_at,
            updated_at: installed_at,
            dependencies: Vec::new(),
        }))
    }

    async fn check_updates(&self) -> PackageManagerResult<Vec<(String, String, String)>> {
        // Simplified - return empty for now
        Ok(Vec::new())
    }
}
