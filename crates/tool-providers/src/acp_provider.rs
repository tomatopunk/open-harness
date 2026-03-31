//! ACP (Agent Client Protocol) tool provider for invoking external ACP-compatible agents
//!
//! This module provides integration with external ACP agents, allowing the Harness agent
//! to delegate tasks to specialized external agents.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use agent_ports::{
    HealthStatus, PortError, PortResult, ToolCallSpec, ToolManifest, ToolProvider,
    ToolProviderType, ToolResult, ToolResultMetadata,
};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Instant;
use tracing::{debug, info, warn};

use unified_config::ACPAgentsConfig;

/// ACP tool provider configuration
#[derive(Debug, Clone)]
pub struct AcpToolProviderConfig {
    /// ACP agents configuration
    pub agents_config: ACPAgentsConfig,
    /// Base workspace directory for ACP agents
    pub workspace_base: String,
    /// MCP servers configuration (optional)
    pub mcp_servers: Option<Value>,
}

/// ACP tool provider that invokes external ACP-compatible agents
pub struct AcpToolProvider {
    config: AcpToolProviderConfig,
}

impl AcpToolProvider {
    /// Create a new ACP tool provider
    pub fn new(config: AcpToolProviderConfig) -> Self {
        Self { config }
    }

    /// Build the tool manifest for invoke_acp_agent
    pub fn manifest(&self) -> ToolManifest {
        let agent_names: Vec<String> =
            self.config.agents_config.agent_names().iter().map(|s| s.to_string()).collect();

        ToolManifest {
            name: "invoke_acp_agent".to_string(),
            description: Some("Invoke an external ACP-compatible agent".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": { "type": "string" },
                    "prompt": { "type": "string" }
                },
                "required": ["agent", "prompt"]
            })),
            capability_tags: vec![],
            risk_level: agent_ports::RiskLevel::Medium,
            timeout_ms: 300000,
            retry_max: 0,
            side_effect_class: agent_ports::SideEffectClass::Write,
            provider_type: ToolProviderType::Local,
            provider_name: "acp".to_string(),
            load_path: None,
            version: Some("1.0.0".to_string()),
        }
    }
}

#[async_trait]
impl ToolProvider for AcpToolProvider {
    fn provider_type(&self) -> ToolProviderType {
        ToolProviderType::Local
    }

    fn provider_name(&self) -> &str {
        "acp"
    }

    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>> {
        if self.config.agents_config.agents.is_empty() {
            return Ok(vec![]);
        }

        Ok(vec![self.manifest()])
    }

    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult> {
        let start = Instant::now();

        if call.name != "invoke_acp_agent" {
            return Err(PortError::NotFound(format!("Unknown ACP tool: {}", call.name)));
        }

        // Parse arguments
        let args = &call.args;
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PortError::Tool("Missing or invalid 'agent' parameter".to_string()))?;
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PortError::Tool("Missing or invalid 'prompt' parameter".to_string()))?;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        // Return placeholder result for now
        Ok(ToolResult {
            success: true,
            data: serde_json::json!({
                "output": format!("ACP agent {} invocation placeholder", agent_name),
                "prompt_preview": &prompt[..std::cmp::min(100, prompt.len())]
            }),
            metadata: ToolResultMetadata {
                tool_name: call.name.clone(),
                provider_type: self.provider_type(),
                provider_name: self.provider_name().to_string(),
                execution_time_ms,
                retries: 0,
                error_message: None,
            },
        })
    }

    async fn health_check(&self) -> PortResult<HealthStatus> {
        if self.config.agents_config.agents.is_empty() {
            return Ok(HealthStatus::Unhealthy("No ACP agents configured".to_string()));
        }

        Ok(HealthStatus::Healthy)
    }
}

/// Builder for AcpToolProvider
pub struct AcpToolProviderBuilder {
    agents_config: Option<ACPAgentsConfig>,
    workspace_base: Option<String>,
    mcp_servers: Option<Value>,
}

impl AcpToolProviderBuilder {
    pub fn new() -> Self {
        Self { agents_config: None, workspace_base: None, mcp_servers: None }
    }

    pub fn with_agents_config(mut self, config: ACPAgentsConfig) -> Self {
        self.agents_config = Some(config);
        self
    }

    pub fn with_workspace_base(mut self, path: String) -> Self {
        self.workspace_base = Some(path);
        self
    }

    pub fn with_mcp_servers(mut self, servers: Option<Value>) -> Self {
        self.mcp_servers = servers;
        self
    }

    pub fn build(self) -> Result<AcpToolProvider, String> {
        let agents_config =
            self.agents_config.ok_or_else(|| "Agents config is required".to_string())?;
        let workspace_base =
            self.workspace_base.ok_or_else(|| "Workspace base is required".to_string())?;

        Ok(AcpToolProvider::new(AcpToolProviderConfig {
            agents_config,
            workspace_base,
            mcp_servers: self.mcp_servers,
        }))
    }
}

impl Default for AcpToolProviderBuilder {
    fn default() -> Self {
        Self::new()
    }
}
