use thiserror::Error;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("docker: {0}")]
    Docker(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("execution: {0}")]
    Execution(String),
    #[error("timeout")]
    Timeout,
}
