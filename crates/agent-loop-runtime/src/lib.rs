//! Multi-turn **model → tool → state** agent loop built on `agent-ports` and `graph-runtime-core`.

pub mod budget;
pub mod error;
pub mod loop_engine;

pub use budget::RunBudget;
pub use error::{AgentLoopError, AgentLoopResult};
pub use loop_engine::{run_agent_loop, AgentLoopDeps, ToolLoopConfig};
