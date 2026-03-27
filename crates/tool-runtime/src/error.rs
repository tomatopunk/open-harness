use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("timeout")]
    Timeout,
    #[error("validation: {0}")]
    Validation(String),
    #[error("execution: {0}")]
    Execution(String),
}
