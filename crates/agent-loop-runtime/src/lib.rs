//! Multi-turn **model → tool → state** agent loop built on `agent-ports` and `graph-runtime-core`.
//!
//! Runtime layout and production contracts: see `ARCHITECTURE.md` in this crate.

mod agent_loop_types;
pub mod budget;
mod commit_metadata;
mod dispatch;
mod engine_v2;
pub mod error;
mod lead_kernel;
mod loop_common;
pub mod loop_engine;
mod loop_hardening;
pub mod middleware;
pub mod policy;
pub mod pregel;
pub mod run_config;
pub mod runtime_spec;
mod scheduler;
pub mod state_patch;
pub mod state_reducer;
pub mod superstep_kernel;
mod superstep_turn;
mod turn_flow;
mod turn_reducer;

pub use agent_ports::EngineCommand;
pub use budget::{truncate_subtask_plan, RunBudget};
pub use error::{AgentLoopError, AgentLoopResult};
pub use loop_engine::{run_agent_loop, AgentLoopDeps, ToolLoopConfig};
pub use middleware::{AgentLoopMiddleware, MiddlewareChain, NoopMiddleware, TurnContext};
pub use run_config::AgentLoopRunConfig;
pub use runtime_spec::{LeadRuntimeSpec, SubagentRuntimeSpec};
pub use turn_flow::{classify_turn_outcome, TurnOutcome};
