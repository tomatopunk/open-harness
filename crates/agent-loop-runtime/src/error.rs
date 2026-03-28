use agent_ports::PortError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentLoopError {
    #[error("port: {0}")]
    Port(#[from] PortError),
    #[error("graph: {0}")]
    Graph(String),
    #[error("max_turns_exceeded")]
    MaxTurnsExceeded,
}

pub type AgentLoopResult<T> = Result<T, AgentLoopError>;
