use agent_ports::{
    HealthStatus, PortError, PortResult, ToolCallSpec, ToolManifest, ToolProvider,
    ToolProviderType, ToolResult, ToolResultMetadata,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;
use tool_runtime::registry::ToolRegistry;
use tracing::debug;

/// 本地工具提供者
pub struct LocalToolProvider {
    registry: Arc<ToolRegistry>,
    manifests: Vec<ToolManifest>,
}

impl LocalToolProvider {
    pub fn new(registry: Arc<ToolRegistry>, manifests: Vec<ToolManifest>) -> Self {
        Self { registry, manifests }
    }

    /// 从配置创建提供者
    pub fn from_registry(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            manifests: vec![], // 需要从其他地方获取 manifests
        }
    }
}

#[async_trait]
impl ToolProvider for LocalToolProvider {
    fn provider_type(&self) -> ToolProviderType {
        ToolProviderType::Local
    }

    fn provider_name(&self) -> &str {
        "local"
    }

    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>> {
        debug!("Listed {} local tools", self.manifests.len());
        Ok(self.manifests.clone())
    }

    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult> {
        let start = Instant::now();

        let result = self
            .registry
            .invoke_with_timeout(&call.name, call.args.clone(), std::time::Duration::from_secs(30))
            .await
            .map_err(|e| PortError::Tool(format!("Local tool {} failed: {}", call.name, e)))?;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            success: true,
            data: result,
            metadata: ToolResultMetadata {
                tool_name: call.name.clone(),
                provider_type: ToolProviderType::Local,
                provider_name: "local".into(),
                execution_time_ms,
                retries: 0,
                error_message: None,
            },
        })
    }

    async fn health_check(&self) -> PortResult<HealthStatus> {
        // 简单检查：尝试列出工具
        match self.list_tools().await {
            Ok(_) => Ok(HealthStatus::Healthy),
            Err(e) => Ok(HealthStatus::Unhealthy(e.to_string())),
        }
    }
}
