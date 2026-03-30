//! Tool System Integration Example
//!
//! This example demonstrates how to use the new unified tool system with:
//! - MCP tool providers
//! - Skill providers
//! - Local tool providers
//! - Tool discovery and assembly
//! - Middleware chain

use agent_ports::{ToolAssemblyPolicy, ToolProvider};
use mcp_client::load_mcp_servers_from_file;
use skill_system::SkillLoader;
use std::path::PathBuf;
use std::sync::Arc;
use tool_providers::{
    load_mcp_providers, load_skill_provider, LocalToolProvider, McpToolProvider,
    SkillToolProvider, ToolProviderDiscovery,
};
use tool_runtime::ToolRegistry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tool System Integration Example ===\n");

    // 1. 创建工具提供者发现引擎
    let governance_root = PathBuf::from("governance");
    let discovery = Arc::new(ToolProviderDiscovery::new(Some(&governance_root)));

    // 2. 加载 MCP 提供者
    println!("Loading MCP providers...");
    let mcp_config_path = governance_root.join("mcp.yaml");
    if mcp_config_path.exists() {
        match load_mcp_providers(&discovery, &mcp_config_path).await {
            Ok(_) => println!("✓ MCP providers loaded"),
            Err(e) => println!("✗ Failed to load MCP providers: {}", e),
        }
    } else {
        println!("⊘ MCP config not found, skipping MCP providers");
    }

    // 3. 加载 Skill 提供者
    println!("\nLoading Skill providers...");
    let skills_root = governance_root.join("skills");
    if skills_root.exists() {
        match load_skill_provider(&discovery, &skills_root).await {
            Ok(_) => println!("✓ Skill providers loaded"),
            Err(e) => println!("✗ Failed to load Skill providers: {}", e),
        }
    } else {
        println!("⊘ Skills root not found, skipping Skill providers");
    }

    // 4. 加载本地工具提供者
    println!("\nLoading Local tool providers...");
    let mut registry = ToolRegistry::new();
    // 注册示例工具
    registry.register(Box::new(EchoTool));

    let manifests = vec![create_echo_manifest()];
    let local_provider = LocalToolProvider::new(Arc::new(registry), manifests);
    discovery.add_provider(Arc::new(local_provider)).await;
    println!("✓ Local tool providers loaded");

    // 5. 获取所有工具清单
    println!("\n=== Available Tools ===");
    let all_manifests = discovery.all_manifests().await?;
    for manifest in &all_manifests {
        println!(
            "  - {} (provider: {}, type: {:?})",
            manifest.name, manifest.provider_name, manifest.provider_type
        );
    }
    println!("Total: {} tools", all_manifests.len());

    // 6. 工具装配示例
    println!("\n=== Tool Assembly Example ===");
    let policy = ToolAssemblyPolicy {
        allowed_tags: vec!["builtin".into(), "mcp".into()].into_iter().collect(),
        denied_tools: vec![].into_iter().collect(),
        max_tools: 10,
        allow_high_risk: false,
        ..Default::default()
    };

    let assembled = policy.resolve(&all_manifests);
    println!("Assembled {} tools (filtered from {})", assembled.len(), all_manifests.len());
    for manifest in &assembled {
        println!("  - {}", manifest.name);
    }

    // 7. 健康检查
    println!("\n=== Health Check ===");
    let health_status = discovery.health_check_all().await;
    for (provider_name, status) in health_status {
        match status {
            agent_ports::HealthStatus::Healthy => {
                println!("  ✓ {}: Healthy", provider_name)
            }
            agent_ports::HealthStatus::Unhealthy(e) => {
                println!("  ✗ {}: Unhealthy ({})", provider_name, e)
            }
            agent_ports::HealthStatus::Degraded(e) => {
                println!("  ⚠ {}: Degraded ({})", provider_name, e)
            }
        }
    }

    println!("\n=== Example Complete ===");
    Ok(())
}

/// 示例 Echo 工具
struct EchoTool;

#[async_trait::async_trait]
impl tool_runtime::registry::Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, tool_runtime::ToolError> {
        Ok(args)
    }
}

/// 创建 Echo 工具的 Manifest
fn create_echo_manifest() -> agent_ports::ToolManifest {
    agent_ports::ToolManifest {
        name: "echo".into(),
        description: Some("Echo JSON arguments".into()),
        input_schema: None,
        capability_tags: vec!["builtin".into()],
        risk_level: agent_ports::RiskLevel::Low,
        timeout_ms: 30000,
        retry_max: 0,
        side_effect_class: agent_ports::SideEffectClass::None,
        provider_type: agent_ports::ToolProviderType::Local,
        provider_name: "local".into(),
        load_path: None,
        version: None,
    }
}
