//! Package manager integration for Open Harness ecosystem
//!
//! This crate provides package management functionality for the
//! Open Harness ecosystem, supporting installation, updating, and
//! uninstallation of MCP servers, skills, and plugins through
//! multiple package managers (npm, cargo, pip).

#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]

pub mod manager;
pub mod providers;
pub mod sandbox;
pub mod traits;
pub mod types;

pub use manager::PackageManager;
pub use sandbox::PackageSandbox;
pub use traits::PackageProvider;
pub use types::{
    CargoConfig, InstallRequest, InstallResult, InstalledPackage, NpmConfig, PackageManagerConfig,
    PackageManagerError, PackageManagerResult, PackageType, PipConfig,
};
