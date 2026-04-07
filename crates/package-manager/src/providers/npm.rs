//! npm package provider

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

use crate::traits::PackageProvider;
use crate::types::{
    InstallRequest, InstallResult, InstalledPackage, NpmConfig, PackageManagerError,
    PackageManagerResult, PackageType,
};

/// npm package provider
pub struct NpmProvider {
    config: NpmConfig,
    install_root: PathBuf,
}

impl NpmProvider {
    /// Create new npm provider
    pub fn new(config: NpmConfig, install_root: PathBuf) -> Self {
        Self { config, install_root }
    }

    /// Get npm executable path
    fn npm_path(&self) -> &str {
        self.config.executable.as_deref().unwrap_or("npm")
    }
}

#[async_trait]
impl PackageProvider for NpmProvider {
    fn name(&self) -> &str {
        "npm"
    }

    async fn check_availability(&self) -> PackageManagerResult<bool> {
        if !self.config.enabled {
            return Ok(false);
        }

        let output = Command::new(self.npm_path()).arg("--version").output().await;

        Ok(output.is_ok())
    }

    async fn install(&self, request: &InstallRequest) -> PackageManagerResult<InstallResult> {
        let package_spec = if let Some(version) = &request.version {
            format!("{}@{}", request.name, version)
        } else {
            request.name.clone()
        };

        let install_dir = request
            .install_dir
            .clone()
            .unwrap_or_else(|| self.install_root.join("npm").join(&request.name));

        // Create directory if it doesn't exist
        if !install_dir.exists() {
            tokio::fs::create_dir_all(&install_dir)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to create dir: {}", e)))?;
        }

        let mut command = Command::new(self.npm_path());
        command
            .arg("install")
            .arg(&package_spec)
            .current_dir(&install_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(registry) = &self.config.registry {
            command.arg("--registry").arg(registry);
        }

        tracing::debug!("Running npm install: {:?}", command);

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!("Failed to run npm install: {}", e))
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

        // Get installed version from package.json
        let pkg_json_path =
            install_dir.join("node_modules").join(&request.name).join("package.json");
        let installed_version = if pkg_json_path.exists() {
            let content = tokio::fs::read_to_string(&pkg_json_path).await.map_err(|e| {
                PackageManagerError::Io(format!("Failed to read package.json: {}", e))
            })?;
            let pkg: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                PackageManagerError::Serialization(format!("Failed to parse package.json: {}", e))
            })?;
            pkg.get("version").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        };

        Ok(InstallResult {
            success: true,
            installed_path: Some(install_dir),
            installed_version,
            warnings: Vec::new(),
            error: None,
        })
    }

    async fn uninstall(&self, name: &str) -> PackageManagerResult<()> {
        let install_path = self.install_root.join("npm").join(name);
        if install_path.exists() {
            tokio::fs::remove_dir_all(&install_path)
                .await
                .map_err(|e| PackageManagerError::Io(format!("Failed to uninstall: {}", e)))?;
        }
        Ok(())
    }

    async fn update(&self, name: &str) -> PackageManagerResult<InstallResult> {
        let install_dir = self.install_root.join("npm").join(name);
        if !install_dir.exists() {
            return Err(PackageManagerError::PackageNotFound(name.to_string()));
        }

        let mut command = Command::new(self.npm_path());
        command
            .arg("update")
            .arg(name)
            .current_dir(&install_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = command.output().await.map_err(|e| {
            PackageManagerError::CommandFailed(format!("Failed to run npm update: {}", e))
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
        let npm_root = self.install_root.join("npm");
        if !npm_root.exists() {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();
        let mut entries = tokio::fs::read_dir(&npm_root)
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
        let install_path = self.install_root.join("npm").join(name);
        if !install_path.exists() {
            return Ok(None);
        }

        let pkg_json_path = install_path.join("node_modules").join(name).join("package.json");
        if !pkg_json_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&pkg_json_path)
            .await
            .map_err(|e| PackageManagerError::Io(format!("Failed to read package.json: {}", e)))?;

        let pkg: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            PackageManagerError::Serialization(format!("Failed to parse package.json: {}", e))
        })?;

        let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        let installed_at = std::fs::metadata(&pkg_json_path)
            .ok()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Some(InstalledPackage {
            name: name.to_string(),
            package_type: PackageType::Npm,
            version,
            install_path,
            installed_at,
            updated_at: installed_at,
            dependencies: Vec::new(),
        }))
    }

    async fn check_updates(&self) -> PackageManagerResult<Vec<(String, String, String)>> {
        let packages = self.list_installed().await?;
        let mut updates = Vec::new();

        for pkg in packages {
            let mut command = Command::new(self.npm_path());
            command
                .arg("outdated")
                .arg("--json")
                .current_dir(&pkg.install_path)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());

            if let Ok(output) = command.output().await {
                if output.status.code() == Some(1) {
                    // npm outdated exits with 1 when there are updates
                    if let Ok(outdated) =
                        serde_json::from_slice::<serde_json::Value>(&output.stdout)
                    {
                        if let Some(info) = outdated.get(&pkg.name) {
                            let current = info.get("current").and_then(|v| v.as_str());
                            let latest = info.get("latest").and_then(|v| v.as_str());
                            if let (Some(current), Some(latest)) = (current, latest) {
                                updates.push((
                                    pkg.name.clone(),
                                    current.to_string(),
                                    latest.to_string(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        Ok(updates)
    }
}
