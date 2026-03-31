use agent_ports::{
    HealthStatus, PortError, PortResult, RiskLevel, SideEffectClass, ToolCallSpec, ToolManifest,
    ToolProvider, ToolProviderType, ToolResult, ToolResultMetadata,
};
use async_trait::async_trait;
use mcp_client::{McpTool as McpToolInfo, MultiServerMcpClient};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error};

/// MCP 工具提供者
pub struct McpToolProvider {
    client: Arc<MultiServerMcpClient>,
    server_name: String,
}

impl McpToolProvider {
    pub async fn new(config: mcp_client::McpServerConfig) -> PortResult<Self> {
        let server_name = config.name.clone();
        let client = Arc::new(
            MultiServerMcpClient::new(vec![config], None)
                .await
                .map_err(|e| PortError::Tool(format!("Failed to create MCP client: {}", e)))?,
        );

        Ok(Self { client, server_name })
    }

    /// 从多个服务器配置创建提供者
    pub async fn new_multi(configs: Vec<mcp_client::McpServerConfig>) -> PortResult<Self> {
        let server_name = "multi-mcp".to_string();
        let client = Arc::new(
            MultiServerMcpClient::new(configs, None)
                .await
                .map_err(|e| PortError::Tool(format!("Failed to create MCP client: {}", e)))?,
        );

        Ok(Self { client, server_name })
    }
}

#[async_trait]
impl ToolProvider for McpToolProvider {
    fn provider_type(&self) -> ToolProviderType {
        ToolProviderType::Mcp
    }

    fn provider_name(&self) -> &str {
        &self.server_name
    }

    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>> {
        let mcp_tools = self
            .client
            .get_tools()
            .await
            .map_err(|e| PortError::Tool(format!("Failed to get MCP tools: {}", e)))?;

        let manifests: Vec<ToolManifest> =
            mcp_tools.into_iter().map(|t| mcp_tool_to_manifest(t, &self.server_name)).collect();

        debug!("Listed {} MCP tools", manifests.len());
        Ok(manifests)
    }

    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult> {
        let start = Instant::now();

        let result = self.client.call_tool(&call.name, &call.args).await.map_err(|e| {
            error!("MCP tool {} failed: {}", call.name, e);
            PortError::Tool(format!("MCP tool {} failed: {}", call.name, e))
        })?;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            success: true,
            data: result,
            metadata: ToolResultMetadata {
                tool_name: call.name.clone(),
                provider_type: ToolProviderType::Mcp,
                provider_name: self.server_name.clone(),
                execution_time_ms,
                retries: 0,
                error_message: None,
            },
        })
    }

    async fn health_check(&self) -> PortResult<HealthStatus> {
        // Convert mcp_client::HealthStatus to agent_ports::HealthStatus
        match self.client.health_check().await {
            mcp_client::HealthStatus::Healthy => Ok(HealthStatus::Healthy),
            mcp_client::HealthStatus::Unhealthy(e) => Ok(HealthStatus::Unhealthy(e)),
            mcp_client::HealthStatus::Degraded(e) => Ok(HealthStatus::Degraded(e)),
        }
    }
}

/// 将 MCP 工具转换为 ToolManifest
fn mcp_tool_to_manifest(tool: McpToolInfo, provider_name: &str) -> ToolManifest {
    ToolManifest {
        name: tool.name,
        description: tool.description,
        input_schema: Some(tool.input_schema),
        capability_tags: vec!["mcp".into()],
        risk_level: RiskLevel::Medium,
        timeout_ms: 60000,
        retry_max: 1,
        side_effect_class: SideEffectClass::Network,
        provider_type: ToolProviderType::Mcp,
        provider_name: provider_name.to_string(),
        load_path: None,
        version: None,
    }
}
