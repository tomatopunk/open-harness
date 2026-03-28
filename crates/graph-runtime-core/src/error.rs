use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphRuntimeError {
    #[error("thread not found: {0}")]
    ThreadNotFound(String),
    #[error("run not found: {0}")]
    RunNotFound(String),
    #[error("checkpoint: {0}")]
    Checkpoint(String),
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
}

pub type GraphResult<T> = Result<T, GraphRuntimeError>;
