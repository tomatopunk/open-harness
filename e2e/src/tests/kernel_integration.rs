mod common;

use agent_kernel::{AgentKernel, KernelConfig};
use std::path::PathBuf;
use tempfile::tempdir;
use tokio;

#[tokio::test]
async fn test_kernel_initialize_start_and_stop_succeeds() {
    let temp_dir = tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    let mut config = KernelConfig::default();
    config.workspace_root = workspace_root.clone();
    config.plugins_dir = workspace_root.join("plugins");

    let kernel = AgentKernel::new(config);

    assert!(kernel.llm_provider().is_none());
    assert!(kernel.mcp_bridge().is_none());
    assert!(kernel.agent_loop().is_none());
    assert!(kernel.memory_system().is_none());

    kernel.initialize().await.unwrap();

    assert!(kernel.llm_provider().is_some());
    assert!(kernel.mcp_bridge().is_some());
    assert!(kernel.agent_loop().is_some());
    assert!(kernel.memory_system().is_some());
    assert!(kernel.channel_manager().is_some());

    kernel.start().await.unwrap();
    kernel.stop().await.unwrap();
}

#[tokio::test]
async fn test_kernel_config_defaults_are_sane() {
    let config = KernelConfig::default();

    assert_eq!(config.log_level, "info");
    assert_eq!(config.agent_loop.enabled, true);
    assert_eq!(config.agent_loop.max_iterations, 100);
    assert_eq!(config.memory.enabled, true);
    assert!(config.gateway.is_some());
    assert!(config.manage.is_some());
}

#[tokio::test]
async fn test_kernel_event_bus_is_accessible() {
    let temp_dir = tempdir().unwrap();
    let mut config = KernelConfig::default();
    config.workspace_root = temp_dir.path().to_path_buf();
    config.plugins_dir = temp_dir.path().join("plugins");
    let kernel = AgentKernel::new(config);

    let _event_bus = kernel.event_bus();
}

#[tokio::test]
async fn test_kernel_hook_system_is_accessible() {
    let temp_dir = tempdir().unwrap();
    let mut config = KernelConfig::default();
    config.workspace_root = temp_dir.path().to_path_buf();
    config.plugins_dir = temp_dir.path().join("plugins");
    let kernel = AgentKernel::new(config);

    let _hooks = kernel.hooks();
}

#[test]
fn test_checked_in_runtime_configuration_resolves_against_governance_models() {
    let repo_root = common::checked_in_runtime_root();
    let config_path = repo_root.join("config.yaml");

    let config = KernelConfig::resolve_runtime(&config_path).unwrap();

    assert_eq!(config.llm.model, "gpt-4");
    assert_eq!(config.extensions_config_path.as_deref(), Some("./extensions_config.json"));
    assert_eq!(config.mcp.servers.len(), 3);
    assert_eq!(config.mcp.servers[0].name, "filesystem");
}

#[test]
fn test_kernel_config_from_missing_file_returns_default() {
    let non_existent_path = PathBuf::from("/this/path/does/not/exist/config.yaml");
    let config = KernelConfig::from_file(&non_existent_path).unwrap();

    assert_eq!(config.log_level, "info");
}
