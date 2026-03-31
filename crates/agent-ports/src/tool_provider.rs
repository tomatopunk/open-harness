use crate::error::PortResult;
use crate::tool_manifest::ToolManifest;
use crate::tool_manifest::ToolProviderType;
use crate::ToolCallSpec;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具调用结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub data: Value,
    pub metadata: ToolResultMetadata,
}

/// 工具结果元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMetadata {
    pub tool_name: String,
    pub provider_type: ToolProviderType,
    pub provider_name: String,
    pub execution_time_ms: u64,
    pub retries: u32,
    pub error_message: Option<String>,
}

/// 健康状态
#[derive(Debug, Clone)]
pub enum HealthStatus {
    Healthy,
    Unhealthy(String),
    Degraded(String),
}

/// 工具提供者 trait
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// 提供者类型
    fn provider_type(&self) -> ToolProviderType;

    /// 提供者名称
    fn provider_name(&self) -> &str;

    /// 列出所有可用工具
    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>>;

    /// 调用工具
    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult>;

    /// 健康检查
    async fn health_check(&self) -> PortResult<HealthStatus>;
}
