use thiserror::Error;

pub type KernelResult<T> = Result<T, KernelError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelErrorCategory {
    Config,
    Initialization,
    Runtime,
    ExternalConnection,
}

#[derive(Error, Debug)]
pub enum KernelError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Plugin error: {source}")]
    Plugin {
        category: KernelErrorCategory,
        #[source]
        source: plugin_system::PluginError,
    },

    #[error("MCP bridge error: {source}")]
    McpBridge {
        category: KernelErrorCategory,
        #[source]
        source: mcp_bridge::McpBridgeError,
    },

    #[error("State error: {source}")]
    State {
        category: KernelErrorCategory,
        #[source]
        source: state_abstraction::StateError,
    },

    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    #[error("Event error: {0}")]
    Event(String),

    #[error("Initialization error: {0}")]
    Initialization(String),

    #[error("External connection error: {0}")]
    ExternalConnection(String),

    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<KernelError>,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Any error: {0}")]
    Any(#[from] anyhow::Error),
}

impl KernelError {
    pub fn category(&self) -> KernelErrorCategory {
        match self {
            Self::Config(_) | Self::Serde(_) => KernelErrorCategory::Config,
            Self::Initialization(_) => KernelErrorCategory::Initialization,
            Self::Plugin { category, .. }
            | Self::McpBridge { category, .. }
            | Self::State { category, .. } => *category,
            Self::Lifecycle(_) | Self::Event(_) | Self::Any(_) => KernelErrorCategory::Runtime,
            Self::ExternalConnection(_) | Self::Io(_) => KernelErrorCategory::ExternalConnection,
            Self::Context { source, .. } => source.category(),
        }
    }

    pub fn context(context: impl Into<String>, source: impl Into<KernelError>) -> Self {
        Self::Context { context: context.into(), source: Box::new(source.into()) }
    }
}

// Convert PluginError to KernelError
impl From<plugin_system::PluginError> for KernelError {
    fn from(err: plugin_system::PluginError) -> Self {
        KernelError::Plugin {
            category: match err.category() {
                plugin_system::PluginErrorCategory::Config => KernelErrorCategory::Config,
                plugin_system::PluginErrorCategory::Initialization => {
                    KernelErrorCategory::Initialization
                }
                plugin_system::PluginErrorCategory::Runtime => KernelErrorCategory::Runtime,
                plugin_system::PluginErrorCategory::ExternalConnection => {
                    KernelErrorCategory::ExternalConnection
                }
            },
            source: err,
        }
    }
}

// Convert McpBridgeError to KernelError
impl From<mcp_bridge::McpBridgeError> for KernelError {
    fn from(err: mcp_bridge::McpBridgeError) -> Self {
        KernelError::McpBridge {
            category: match err.category() {
                mcp_bridge::McpBridgeErrorCategory::Config => KernelErrorCategory::Config,
                mcp_bridge::McpBridgeErrorCategory::Initialization => {
                    KernelErrorCategory::Initialization
                }
                mcp_bridge::McpBridgeErrorCategory::Runtime => KernelErrorCategory::Runtime,
                mcp_bridge::McpBridgeErrorCategory::ExternalConnection => {
                    KernelErrorCategory::ExternalConnection
                }
            },
            source: err,
        }
    }
}

impl From<state_abstraction::StateError> for KernelError {
    fn from(err: state_abstraction::StateError) -> Self {
        KernelError::State {
            category: match err.category() {
                state_abstraction::StateErrorCategory::Config => KernelErrorCategory::Config,
                state_abstraction::StateErrorCategory::Initialization => {
                    KernelErrorCategory::Initialization
                }
                state_abstraction::StateErrorCategory::Runtime => KernelErrorCategory::Runtime,
                state_abstraction::StateErrorCategory::ExternalConnection => {
                    KernelErrorCategory::ExternalConnection
                }
            },
            source: err,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_system::{PluginError, PluginLifecycleStage};
    use state_abstraction::StateError;

    #[test]
    fn kernel_maps_downstream_categories_to_top_level_contract() {
        let plugin_error = PluginError::InvalidConfiguration {
            plugin: "gateway".to_string(),
            details: "missing bind address".to_string(),
        };
        assert_eq!(KernelError::from(plugin_error).category(), KernelErrorCategory::Config);

        let mcp_error = mcp_bridge::McpBridgeError::Server("timeout".to_string());
        assert_eq!(
            KernelError::from(mcp_error).category(),
            KernelErrorCategory::ExternalConnection
        );

        let state_error = StateError::Initialization("storage bootstrap failed".to_string());
        assert_eq!(KernelError::from(state_error).category(), KernelErrorCategory::Initialization);
    }

    #[test]
    fn kernel_context_preserves_nested_downstream_sources() {
        let error = KernelError::context(
            "Failed to discover plugins",
            PluginError::LifecycleFailed {
                plugin: "gateway".to_string(),
                stage: PluginLifecycleStage::Load,
                source: Box::new(PluginError::InvalidConfiguration {
                    plugin: "gateway".to_string(),
                    details: "missing bind address".to_string(),
                }),
            },
        );

        assert_eq!(error.category(), KernelErrorCategory::Config);
        assert!(error.to_string().contains("Failed to discover plugins"));

        match error {
            KernelError::Context { context, source } => {
                assert_eq!(context, "Failed to discover plugins");
                match *source {
                    KernelError::Plugin { category, source } => {
                        assert_eq!(category, KernelErrorCategory::Config);
                        match source {
                            PluginError::LifecycleFailed { plugin, stage, source } => {
                                assert_eq!(plugin, "gateway");
                                assert_eq!(stage, PluginLifecycleStage::Load);
                                match source.as_ref() {
                                    PluginError::InvalidConfiguration { plugin, details } => {
                                        assert_eq!(plugin, "gateway");
                                        assert!(details.contains("missing bind address"));
                                    }
                                    other => panic!("unexpected nested source: {other:?}"),
                                }
                            }
                            other => panic!("unexpected plugin error: {other:?}"),
                        }
                    }
                    other => panic!("unexpected kernel source: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
