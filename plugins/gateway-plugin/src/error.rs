use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayPluginError {
    #[error("API error: {0}")]
    Api(String),

    #[error("Configuration error: {0}")]
    Config(String),
}
