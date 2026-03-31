//! Package provider trait

use crate::types::{InstallRequest, InstallResult, InstalledPackage, PackageManagerResult};
use async_trait::async_trait;

/// Package provider trait
#[async_trait]
pub trait PackageProvider: Send + Sync + 'static {
    /// Get provider name
    fn name(&self) -> &str;

    /// Check if this provider is available on the system
    async fn check_availability(&self) -> PackageManagerResult<bool>;

    /// Install a package
    async fn install(&self, request: &InstallRequest) -> PackageManagerResult<InstallResult>;

    /// Uninstall a package
    async fn uninstall(&self, name: &str) -> PackageManagerResult<()>;

    /// Update a package to latest version
    async fn update(&self, name: &str) -> PackageManagerResult<InstallResult>;

    /// List installed packages
    async fn list_installed(&self) -> PackageManagerResult<Vec<InstalledPackage>>;

    /// Get information about an installed package
    async fn get_installed(&self, name: &str) -> PackageManagerResult<Option<InstalledPackage>>;

    /// Check for updates to installed packages
    async fn check_updates(&self) -> PackageManagerResult<Vec<(String, String, String)>>;
}
