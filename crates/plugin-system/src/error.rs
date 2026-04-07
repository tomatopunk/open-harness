use thiserror::Error;

use crate::plugin::PluginLifecycleStage;

pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginErrorCategory {
    Config,
    Initialization,
    Runtime,
    ExternalConnection,
}

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Invalid plugin manifest: {0}")]
    InvalidManifest(String),

    #[error("Plugin loading failed: {0}")]
    LoadingFailed(String),

    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Plugin configuration is invalid for {plugin}: {details}")]
    InvalidConfiguration { plugin: String, details: String },

    #[error("Plugin {plugin} failed during {stage}: {source}")]
    LifecycleFailed {
        plugin: String,
        stage: PluginLifecycleStage,
        #[source]
        source: Box<PluginError>,
    },

    #[error("Plugin already loaded: {0}")]
    AlreadyLoaded(String),

    #[error("Plugin not loaded: {0}")]
    NotLoaded(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_yaml::Error),

    #[error("Any error: {0}")]
    Any(#[from] anyhow::Error),
}

impl PluginError {
    pub fn category(&self) -> PluginErrorCategory {
        match self {
            Self::InvalidManifest(_) | Self::InvalidConfiguration { .. } | Self::Serde(_) => {
                PluginErrorCategory::Config
            }
            Self::LoadingFailed(_) | Self::InitializationFailed(_) => {
                PluginErrorCategory::Initialization
            }
            Self::NotFound(_) | Self::AlreadyLoaded(_) | Self::NotLoaded(_) | Self::Any(_) => {
                PluginErrorCategory::Runtime
            }
            Self::LifecycleFailed { source, .. } => source.category(),
            Self::Io(_) => PluginErrorCategory::ExternalConnection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_configuration_maps_to_config_category() {
        let error = PluginError::InvalidConfiguration {
            plugin: "gateway".to_string(),
            details: "missing bind address".to_string(),
        };

        assert_eq!(error.category(), PluginErrorCategory::Config);
    }

    #[test]
    fn lifecycle_failure_preserves_source_category() {
        let error = PluginError::LifecycleFailed {
            plugin: "gateway".to_string(),
            stage: PluginLifecycleStage::Load,
            source: Box::new(PluginError::InvalidConfiguration {
                plugin: "gateway".to_string(),
                details: "missing bind address".to_string(),
            }),
        };

        assert_eq!(error.category(), PluginErrorCategory::Config);

        match error {
            PluginError::LifecycleFailed { plugin, stage, source } => {
                assert_eq!(plugin, "gateway");
                assert_eq!(stage, PluginLifecycleStage::Load);
                match source.as_ref() {
                    PluginError::InvalidConfiguration { plugin, details } => {
                        assert_eq!(plugin, "gateway");
                        assert!(details.contains("missing bind address"));
                    }
                    other => panic!("unexpected source: {other:?}"),
                }
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
