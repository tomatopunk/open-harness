use thiserror::Error;

pub type KernelResult<T> = Result<T, KernelError>;

#[derive(Error, Debug)]
pub enum KernelError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    #[error("Event error: {0}")]
    Event(String),

    #[error("Initialization error: {0}")]
    Initialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Any error: {0}")]
    Any(#[from] anyhow::Error),
}

// Convert PluginError to KernelError
impl From<plugin_system::PluginError> for KernelError {
    fn from(err: plugin_system::PluginError) -> Self {
        KernelError::Plugin(err.to_string())
    }
}

// Convert McpBridgeError to KernelError
impl From<mcp_bridge::McpBridgeError> for KernelError {
    fn from(err: mcp_bridge::McpBridgeError) -> Self {
        KernelError::Initialization(err.to_string())
    }
}
