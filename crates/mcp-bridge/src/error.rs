use thiserror::Error;

pub type McpBridgeResult<T> = Result<T, McpBridgeError>;

#[derive(Error, Debug)]
pub enum McpBridgeError {
    #[error("MCP client error: {0}")]
    Client(String),

    #[error("Server error: {0}")]
    Server(String),

    #[error("Skill MCP error: {0}")]
    Skill(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Server not found: {0}")]
    NotFound(String),

    #[error("Server already exists: {0}")]
    AlreadyExists(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Any error: {0}")]
    Any(#[from] anyhow::Error),
}
