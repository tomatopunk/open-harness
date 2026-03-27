use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("invalid thread id: {0}")]
    InvalidThreadId(String),
    #[error("invalid run id: {0}")]
    InvalidRunId(String),
    #[error("serialization: {0}")]
    Serde(#[from] serde_json::Error),
}
