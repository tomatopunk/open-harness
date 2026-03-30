//! Multi-turn **model → tool → state** agent loop built on `agent-ports` and `graph-runtime-core`.
//!
//! Runtime layout and production contracts: see `ARCHITECTURE.md` in this crate.

mod agent_loop_types;
pub mod budget;
pub mod budget_tracker;
mod child_run;
mod commit_metadata;
pub mod concurrency;
mod dispatch;
mod engine_v2;
pub mod error;
mod fallback_executor;
mod lead_kernel;
mod lead_outer_superstep;
mod loop_common;
pub mod loop_engine;
mod loop_hardening;
pub mod middleware;
pub mod policy;
pub mod pregel;
mod premodel_phase;
pub mod retry_executor;
pub mod run_config;
pub mod runtime_spec;
mod scheduler;
pub mod state_patch;
pub mod state_reducer;
pub mod superstep_kernel;
mod turn_flow;
mod turn_reducer;

pub use agent_ports::EngineCommand;
pub use budget::{truncate_subtask_plan, RunBudget};
pub use budget::{BudgetConfig, BudgetPreset, RetryConfig};
pub use budget_tracker::{BudgetManager, BudgetTracker, BudgetUtilization, ConcurrencyGuard};
pub use engine_v2::maybe_resume_from_interrupt;
pub use error::{AgentLoopError, AgentLoopResult};
pub use fallback_executor::{FallbackExecutor, FallbackResult};
pub use loop_engine::{run_agent_loop, AgentLoopDeps, ToolLoopConfig};
pub use middleware::{AgentLoopMiddleware, MiddlewareChain, NoopMiddleware, TurnContext};
pub use retry_executor::{classify_common_error, RetryExecutor};
pub use run_config::AgentLoopRunConfig;
pub use runtime_spec::{DispatchPhaseNodes, LeadRuntimeSpec, SubagentRuntimeSpec};
pub use turn_flow::{classify_turn_outcome, TurnOutcome};
