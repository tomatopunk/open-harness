use crate::tool_manifest::ToolManifest;
use crate::ToolCallSpec;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具提供者类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderType {
    Local,
    Mcp,
    Skill,
    Community,
}

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

/// 端口错误类型（从 ports.rs 复制过来）
#[derive(Debug, thiserror::Error)]
pub enum PortError {
    #[error("LLM error: {0}")]
    Llm(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Skill error: {0}")]
    Skill(String),

    #[error("Subagent error: {0}")]
    Subagent(String),

    #[error("Checkpoint error: {0}")]
    Checkpoint(String),

    #[error("Thread state error: {0}")]
    ThreadState(String),

    #[error("Timeout")]
    Timeout,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type PortResult<T> = Result<T, PortError>;

impl From<PortError> for String {
    fn from(e: PortError) -> Self {
        e.to_string()
    }
}
