//! Port-level errors (transport-agnostic).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortError {
    #[error("llm: {0}")]
    Llm(String),
    #[error("tool: {0}")]
    Tool(String),
    #[error("memory: {0}")]
    Memory(String),
    #[error("skill: {0}")]
    Skill(String),
    #[error("subagent: {0}")]
    Subagent(String),
    #[error("checkpoint: {0}")]
    Checkpoint(String),
    #[error("thread_state: {0}")]
    ThreadState(String),
    #[error("budget_exhausted: {0}")]
    BudgetExhausted(String),
    #[error("clarification_required")]
    ClarificationRequired,
    #[error("aborted: {0}")]
    Aborted(String),
}

pub type PortResult<T> = Result<T, PortError>;
