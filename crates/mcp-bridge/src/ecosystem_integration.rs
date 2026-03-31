//! Ecosystem integration for MCP Bridge
//!
//! This module provides integration with the Open Harness ecosystem,
//! enabling search, installation, and management of MCP servers
//! from the open ecosystem.

use crate::config::{EcosystemConfig, McpBridgeConfig, McpServerConfig};
use crate::error::{McpBridgeError, McpBridgeResult};
use crate::skill_mcp::SkillInfo;
use crate::types::ToolManifest;
use ecosystem_registry::{
    ComponentMetadata, ComponentType, MultiRegistryClient, RegistryClient, SearchQuery, SearchResult,
};
use package_manager::{InstallRequest, InstallResult, PackageManager, PackageType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info};

/// MCP server metadata from ecosystem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerMetadata {
    /// Component ID
    pub id: String,
    /// Server name
    pub name: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
    /// Authors
    pub authors: Vec<String>,
    /// Latest version
    pub latest_version: String,
    /// All available versions
    pub versions: Vec<String>,
    /// License
    pub license: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// Homepage
    pub homepage: Option<String>,
    /// Tags
    pub tags: Vec<String>,
    /// Package information
    pub package: ecosystem_registry::PackageInfo,
}

impl From<ComponentMetadata> for McpServerMetadata {
    fn from(meta: ComponentMetadata) -> Self {
        Self {
            id: meta.id,
            name: meta.name,
            display_name: meta.display_name,
            description: meta.description,
            authors: meta.authors,
            latest_version: meta.version,
            versions: meta.versions,
            license: meta.license,
            repository: meta.repository,
            homepage: meta.homepage,
            tags: meta.tags,
            package: meta.package,
        }
    }
}

/// Ecosystem integration for MCP Bridge
pub struct McpEcosystemIntegration {
    /// Ecosystem configuration
    config: EcosystemConfig,
    /// Multi-registry client
    registry_client: Option<MultiRegistryClient>,
    /// Package manager
    package_manager: Option<PackageManager>,
    /// Cache directory
    cache_dir: PathBuf,
}

impl McpEcosystemIntegration {
    /// Create new ecosystem integration
    pub fn new(config: EcosystemConfig, cache_dir: PathBuf) -> Self {
        Self {
            config,
            registry_client: None,
            package_manager: None,
            cache_dir,
        }
    }

    /// Initialize ecosystem integration
    pub async fn initialize(&mut self) -> McpBridgeResult<()> {
        if !self.config.enabled {
            debug!("Ecosystem integration is disabled");
            return Ok(());
        }

        info!("Initializing MCP ecosystem integration...");

        // Initialize multi-registry client
        if !self.config.registries.is_empty() {
            let mut client = MultiRegistryClient::new(
                self.config.registries.clone(),
                self.cache_dir.join("registry"),
            );
            client.initialize().await.map_err(|e| {
                McpBridgeError::Config(format!("Failed to initialize registry client: {}", e))
            })?;
            self.registry_client = Some(client);
        }

        // Initialize package manager
        let pm_config = package_manager::PackageManagerConfig {
            install_root: self.config.package_manager.install_root.clone(),
            sandbox_enabled: self.config.package_manager.sandbox_enabled,
            ..Default::default()
        };
        let mut pm = PackageManager::new(pm_config);
        pm.initialize().await.map_err(|e| {
            McpBridgeError::Config(format!("Failed to initialize package manager: {}", e))
        })?;
        self.package_manager = Some(pm);

        info!("MCP ecosystem integration initialized");
        Ok(())
    }

    /// Search for MCP servers in ecosystem
    pub async fn search_servers(&self, query: &str) -> McpBridgeResult<SearchResult> {
        if let Some(client) = &self.registry_client {
            let search_query = SearchQuery {
                query: Some(query.to_string()),
                component_type: Some(ComponentType::McpServer),
                ..Default::default()
            };

            let result = client.search(&search_query).await.map_err(|e| {
                McpBridgeError::Config(format!("Search failed: {}", e))
            })?;

            Ok(result)
        } else {
            Ok(SearchResult {
                total: 0,
                items: Vec::new(),
                page: 1,
                per_page: 20,
            })
        }
    }

    /// Get MCP server metadata by ID
    pub async fn get_server(&self, id: &str) -> McpBridgeResult<ComponentMetadata> {
        if let Some(client) = &self.registry_client {
            let (_, metadata) = client.find_component(id).await.map_err(|e| {
                McpBridgeError::Config(format!("Failed to get component: {}", e))
            })?;

            if metadata.component_type != ComponentType::McpServer {
                return Err(McpBridgeError::Config(format!(
                    "Component {} is not an MCP server", id
                )));
            }

            Ok(metadata)
        } else {
            Err(McpBridgeError::Config(
                "No registry client configured".to_string(),
            ))
        }
    }

    /// Install MCP server from ecosystem
    pub async fn install_server(
        &self,
        id: &str,
        version: Option<&str>,
    ) -> McpBridgeResult<McpServerConfig> {
        let metadata = self.get_server(id).await?;
        let package_type = match metadata.package.r#type.as_str() {
            "npm" => PackageType::Npm,
            "cargo" => PackageType::Cargo,
            "pip" => PackageType::Pip,
            "git" => PackageType::Git,
            _ => {
                return Err(McpBridgeError::Config(format!(
                    "Unsupported package type: {}", metadata.package.r#type
                )));
            }
        };

        if let Some(pm) = &self.package_manager {
            let install_request = InstallRequest {
                name: metadata.package.name.clone(),
                package_type,
                version: version.map(|v| v.to_string()),
                git_url: metadata.repository.clone(),
                install_dir: None,
                force: false,
            };

            let result: InstallResult = pm.install(&install_request).await.map_err(|e| {
                McpBridgeError::Config(format!("Installation failed: {}", e))
            })?;

            if !result.success {
                return Err(McpBridgeError::Config(format!(
                    "Installation failed: {}",
                    result.error.unwrap_or_default()
                )));
            }

            // Create MCP server config based on package type
            let server_config = self.create_server_config(&metadata, &result)?;

            info!("MCP server {} installed successfully at {:?}", id, result.installed_path);

            Ok(server_config)
        } else {
            Err(McpBridgeError::Config(
                "Package manager not initialized".to_string(),
            ))
        }
    }

    /// Install MCP server directly from GitHub
    pub async fn install_from_github(
        &self,
        repo: &str,
        r#ref: Option<&str>,
    ) -> McpBridgeResult<McpServerConfig> {
        // For GitHub installation, we clone the repository and install
        // This implementation assumes the repository contains an npm/cargo/pip package

        // For simplicity, we use the package manager with git type
        if let Some(pm) = &self.package_manager {
            let install_request = InstallRequest {
                name: repo.to_string(),
                package_type: PackageType::Git,
                version: r#ref.map(|v| v.to_string()),
                git_url: Some(format!("https://github.com/{}.git", repo)),
                install_dir: None,
                force: false,
            };

            let result = pm.install(&install_request).await.map_err(|e| {
                McpBridgeError::Config(format!("GitHub installation failed: {}", e))
            })?;

            if !result.success {
                return Err(McpBridgeError::Config(format!(
                    "GitHub installation failed: {}",
                    result.error.unwrap_or_default()
                )));
            }

            // Try to find skill.yaml or mcp.json in the installed repository
            let install_path = result.installed_path
                .as_ref()
                .ok_or_else(|| McpBridgeError::Config("No install path returned".to_string()))?;

            // Look for skill.yaml
            let skill_yaml = install_path.join("skill.yaml");
            if skill_yaml.exists() {
                let content = std::fs::read_to_string(&skill_yaml).map_err(|e| {
                    McpBridgeError::Io(e)
                })?;
                let skill_info: SkillInfo = serde_yaml::from_str(&content).map_err(|e| {
                    McpBridgeError::Config(format!("Invalid skill.yaml: {}", e))
                })?;

                if let Some(command) = skill_info.mcp_command {
                    let args = skill_info.mcp_args.unwrap_or_default();
                    let env = skill_info.mcp_env.unwrap_or_default();

                    Ok(McpServerConfig {
                        name: format!("github-{}-{}", repo.replace('/', "-"), skill_info.name),
                        command,
                        args,
                        env,
                        enabled: true,
                        is_skill_mcp: true,
                        skill_names: vec![skill_info.name],
                    })
                } else {
                    Err(McpBridgeError::Config(
                        "skill.yaml does not contain mcpCommand".to_string(),
                    ))
                }
            } else {
                // TODO: Try to find mcp configuration
                Err(McpBridgeError::Config(
                    "No skill.yaml found in GitHub repository".to_string(),
                ))
            }
        } else {
            Err(McpBridgeError::Config(
                "Package manager not initialized".to_string(),
            ))
        }
    }

    /// Uninstall MCP server
    pub async fn uninstall_server(&self, name: &str, package_type: PackageType) -> McpBridgeResult<()> {
        if let Some(pm) = &self.package_manager {
            pm.uninstall(package_type, name).await.map_err(|e| {
                McpBridgeError::Config(format!("Uninstall failed: {}", e))
            })?;

            info!("MCP server {} uninstalled successfully", name);
            Ok(())
        } else {
            Err(McpBridgeError::Config(
                "Package manager not initialized".to_string(),
            ))
        }
    }

    /// Update MCP server to latest version
    pub async fn update_server(&self, name: &str, package_type: PackageType) -> McpBridgeResult<()> {
        if let Some(pm) = &self.package_manager {
            pm.update(package_type, name).await.map_err(|e| {
                McpBridgeError::Config(format!("Update failed: {}", e))
            })?;

            info!("MCP server {} updated successfully", name);
            Ok(())
        } else {
            Err(McpBridgeError::Config(
                "Package manager not initialized".to_string(),
            ))
        }
    }

    /// List installed MCP servers
    pub async fn list_installed(&self) -> McpBridgeResult<Vec<(String, String, PathBuf)>> {
        if let Some(pm) = &self.package_manager {
            let packages = pm.list_installed().await.map_err(|e| {
                McpBridgeError::Config(format!("Failed to list installed: {}", e))
            })?;

            let result = packages
                .into_iter()
                .filter(|p| matches!(p.package_type, PackageType::Npm | PackageType::Cargo | PackageType::Pip))
                .map(|p| (p.name, p.version, p.install_path))
                .collect();

            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }

    fn create_server_config(
        &self,
        metadata: &ComponentMetadata,
        result: &InstallResult,
    ) -> McpBridgeResult<McpServerConfig> {
        let install_path = result
            .installed_path
            .as_ref()
            .ok_or_else(|| McpBridgeError::Config("No install path".to_string()))?;

        let (command, args) = match metadata.package.r#type.as_str() {
            "npm" => {
                // For npm packages that provide MCP server
                // The binary is typically in node_modules/.bin
                let bin_path = install_path
                    .join("node_modules")
                    .join(".bin")
                    .join(&metadata.name);
                (bin_path.to_string_lossy().to_string(), Vec::new())
            }
            "cargo" => {
                // For cargo, the binary is after build in target/release
                let bin_path = install_path.join("target").join("release").join(&metadata.name);
                (bin_path.to_string_lossy().to_string(), Vec::new())
            }
            "pip" =>
            {
                // For pip, we typically execute python with the module
                (
                    "python".to_string(),
                    vec!["-m".to_string(), metadata.package.name.clone()],
                )
            }
            _ => (metadata.package.name.clone(), Vec::new()),
        };

        let mut env = metadata.package.env.clone();
        // Add any additional environment variables

        Ok(McpServerConfig {
            name: metadata.id.clone(),
            command,
            args,
            env,
            enabled: true,
            is_skill_mcp: metadata.component_type == ComponentType::Skill,
            skill_names: if metadata.component_type == ComponentType::Skill {
                vec![metadata.name.clone()]
            } else {
                Vec::new()
            },
        })
    }

    /// Check if ecosystem integration is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}
