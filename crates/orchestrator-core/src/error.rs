use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("model: {0}")]
    Model(String),
    #[error("tool: {0}")]
    Tool(String),
    #[error("state: {0}")]
    State(String),
}
