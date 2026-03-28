use agent_ports::PortError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentLoopError {
    #[error("port: {0}")]
    Port(#[from] PortError),
    #[error("graph: {0}")]
    Graph(String),
    #[error("lead_kernel: {0}")]
    LeadKernel(String),
    #[error("max_turns_exceeded")]
    MaxTurnsExceeded,
    #[error("invariant: {0}")]
    InvariantViolation(String),
}

pub type AgentLoopResult<T> = Result<T, AgentLoopError>;
