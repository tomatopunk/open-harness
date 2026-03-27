use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Value { payload: Value },
    ToolCall { invocation: ToolInvocation },
    Message { role: String, content: String },
    End { reason: String },
    Error { message: String },
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("middleware failed: {0}")]
    Middleware(String),
    #[error("subagent failed: {0}")]
    Subagent(String),
    #[error("invalid runtime input: {0}")]
    InvalidInput(String),
}
