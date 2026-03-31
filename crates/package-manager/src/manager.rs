//! Main package manager implementation

#![allow(clippy::await_holding_lock)]

use crate::providers::{cargo::CargoProvider, npm::NpmProvider, pip::PipProvider};
use crate::sandbox::PackageSandbox;
use crate::traits::PackageProvider;
use crate::types::{
    InstallRequest, InstallResult, InstalledPackage, PackageManagerConfig, PackageManagerError,
    PackageManagerResult, PackageType,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

/// Main package manager
pub struct PackageManager {
    config: PackageManagerConfig,
    providers: RwLock<HashMap<PackageType, Box<dyn PackageProvider>>>,
    sandbox: Option<PackageSandbox>,
}

impl PackageManager {
    /// Create new package manager
    pub fn new(config: PackageManagerConfig) -> Self {
        let sandbox = if config.sandbox_enabled {
            Some(PackageSandbox::new(crate::sandbox::SandboxConfig {
                root: config.install_root.join(".sandbox"),
                ..Default::default()
            }))
        } else {
            None
        };

        Self { config, providers: RwLock::new(HashMap::new()), sandbox }
    }

    /// Initialize package manager and all providers
    pub async fn initialize(&mut self) -> PackageManagerResult<()> {
        if !self.config.install_root.exists() {
            tokio::fs::create_dir_all(&self.config.install_root).await.map_err(|e| {
                PackageManagerError::Io(format!("Failed to create install root: {}", e))
            })?;
        }

        if let Some(sandbox) = &mut self.sandbox {
            sandbox.initialize().await?;
        }

        // Initialize npm provider
        let npm_provider =
            NpmProvider::new(self.config.npm.clone(), self.config.install_root.clone());
        if npm_provider.check_availability().await.unwrap_or(false) {
            self.providers.write().insert(PackageType::Npm, Box::new(npm_provider));
            debug!("npm provider initialized");
        } else {
            debug!("npm provider not available");
        }

        // Initialize cargo provider
        let cargo_provider =
            CargoProvider::new(self.config.cargo.clone(), self.config.install_root.clone());
        if cargo_provider.check_availability().await.unwrap_or(false) {
            self.providers.write().insert(PackageType::Cargo, Box::new(cargo_provider));
            debug!("cargo provider initialized");
        } else {
            debug!("cargo provider not available");
        }

        // Initialize pip provider
        let pip_provider =
            PipProvider::new(self.config.pip.clone(), self.config.install_root.clone());
        if pip_provider.check_availability().await.unwrap_or(false) {
            self.providers.write().insert(PackageType::Pip, Box::new(pip_provider));
            debug!("pip provider initialized");
        } else {
            debug!("pip provider not available");
        }

        Ok(())
    }

    /// Install a package
    pub async fn install(&self, request: &InstallRequest) -> PackageManagerResult<InstallResult> {
        let providers = self.providers.read();
        let provider = providers.get(&request.package_type).ok_or_else(|| {
            PackageManagerError::NotAvailable(format!(
                "Package provider {:?} not available",
                request.package_type
            ))
        })?;

        let result = provider.install(request).await?;
        Ok(result)
    }

    /// Uninstall a package
    pub async fn uninstall(
        &self,
        package_type: PackageType,
        name: &str,
    ) -> PackageManagerResult<()> {
        let providers = self.providers.read();
        let provider = providers.get(&package_type).ok_or_else(|| {
            PackageManagerError::NotAvailable(format!(
                "Package provider {:?} not available",
                package_type
            ))
        })?;

        provider.uninstall(name).await?;
        Ok(())
    }

    /// Update a package
    pub async fn update(
        &self,
        package_type: PackageType,
        name: &str,
    ) -> PackageManagerResult<InstallResult> {
        let providers = self.providers.read();
        let provider = providers.get(&package_type).ok_or_else(|| {
            PackageManagerError::NotAvailable(format!(
                "Package provider {:?} not available",
                package_type
            ))
        })?;

        provider.update(name).await?;
        Ok(InstallResult {
            success: true,
            installed_path: None,
            installed_version: None,
            warnings: Vec::new(),
            error: None,
        })
    }

    /// List all installed packages
    pub async fn list_installed(&self) -> PackageManagerResult<Vec<InstalledPackage>> {
        let mut all_packages = Vec::new();
        let providers = self.providers.read();

        for provider in providers.values() {
            match provider.list_installed().await {
                Ok(packages) => {
                    all_packages.extend(packages);
                }
                Err(e) => {
                    tracing::warn!("Failed to list packages from {}: {}", provider.name(), e);
                }
            }
        }

        Ok(all_packages)
    }

    /// Get installed package information
    pub async fn get_installed(
        &self,
        package_type: PackageType,
        name: &str,
    ) -> PackageManagerResult<Option<InstalledPackage>> {
        let providers = self.providers.read();
        let provider = providers.get(&package_type).ok_or_else(|| {
            PackageManagerError::NotAvailable(format!(
                "Package provider {:?} not available",
                package_type
            ))
        })?;

        provider.get_installed(name).await
    }

    /// Check for available updates
    pub async fn check_updates(&self) -> PackageManagerResult<Vec<(String, String, String)>> {
        let mut all_updates = Vec::new();
        let providers = self.providers.read();

        for provider in providers.values() {
            match provider.check_updates().await {
                Ok(updates) => {
                    all_updates.extend(updates);
                }
                Err(e) => {
                    tracing::warn!("Failed to check updates from {}: {}", provider.name(), e);
                }
            }
        }

        Ok(all_updates)
    }

    /// Get available providers
    pub fn available_providers(&self) -> Vec<PackageType> {
        self.providers.read().keys().cloned().collect()
    }

    /// Get install root
    pub fn install_root(&self) -> &PathBuf {
        &self.config.install_root
    }
}
