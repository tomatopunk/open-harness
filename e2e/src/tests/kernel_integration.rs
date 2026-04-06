use agent_kernel::{AgentKernel, KernelConfig};
use std::path::PathBuf;
use tempfile::tempdir;
use tokio;

#[tokio::test]
async fn test_kernel_creation_succeeds() {
    let temp_dir = tempdir().unwrap();
    let workspace_root = temp_dir.path().to_path_buf();

    let mut config = KernelConfig::default();
    config.workspace_root = workspace_root.clone();

    let kernel = AgentKernel::new(config);

    assert!(kernel.llm_provider().is_none());
    assert!(kernel.mcp_bridge().is_none());
    assert!(kernel.agent_loop().is_none());
    assert!(kernel.memory_system().is_none());
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
    let config = KernelConfig::default();
    let kernel = AgentKernel::new(config);

    let _event_bus = kernel.event_bus();
}

#[tokio::test]
async fn test_kernel_hook_system_is_accessible() {
    let config = KernelConfig::default();
    let kernel = AgentKernel::new(config);

    let _hooks = kernel.hooks();
}

#[test]
fn test_kernel_config_from_missing_file_returns_default() {
    let non_existent_path = PathBuf::from("/this/path/does/not/exist/config.yaml");
    let config = KernelConfig::from_file(&non_existent_path).unwrap();

    assert_eq!(config.log_level, "info");
}
