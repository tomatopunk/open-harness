use thiserror::Error;

pub type ProviderResult<T> = Result<T, ProviderError>;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Provider not supported: {0}")]
    UnsupportedProvider(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Any error: {0}")]
    Any(#[from] anyhow::Error),
}
