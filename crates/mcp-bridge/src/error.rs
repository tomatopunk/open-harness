use thiserror::Error;

pub type McpBridgeResult<T> = Result<T, McpBridgeError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpBridgeErrorCategory {
    Config,
    Initialization,
    Runtime,
    ExternalConnection,
}

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

impl McpBridgeError {
    pub fn category(&self) -> McpBridgeErrorCategory {
        match self {
            Self::Config(_) | Self::Serde(_) => McpBridgeErrorCategory::Config,
            Self::Skill(_) => McpBridgeErrorCategory::Initialization,
            Self::NotFound(_) | Self::AlreadyExists(_) | Self::Any(_) => {
                McpBridgeErrorCategory::Runtime
            }
            Self::Client(_) | Self::Server(_) | Self::Io(_) => {
                McpBridgeErrorCategory::ExternalConnection
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_mapping_matches_cross_layer_contract() {
        assert_eq!(
            McpBridgeError::Config("bad config".to_string()).category(),
            McpBridgeErrorCategory::Config
        );
        assert_eq!(
            McpBridgeError::Server("timeout".to_string()).category(),
            McpBridgeErrorCategory::ExternalConnection
        );
        assert_eq!(
            McpBridgeError::NotFound("alpha".to_string()).category(),
            McpBridgeErrorCategory::Runtime
        );
    }
}
