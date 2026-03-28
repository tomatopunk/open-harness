//! Multi-turn model-tool-state loop with explicit `agent_ports::LoopStage` boundaries (V2 engine).
//!
//! # Stage mapping (see `ARCHITECTURE.md` in this crate)
//!
//! | Stage | When |
//! |-------|------|
//! | `LoopStage::PreModel` | After `apply_lead_kernel_turn`: skill injection + memory retrieve. |
//! | `LoopStage::Model` | `LLMPort::infer_turn` only. |
//! | `LoopStage::PostModel` | `AgentLoopMiddleware::after_model` (no `commit_step` here). |
//! | `LoopStage::ClarifyExit` | Pending clarification; checkpoint then return. |
//! | `LoopStage::SubagentExec` | Subagent plan execution + merge. |
//! | `LoopStage::ToolExec` | Tool invocations for one model turn. |
//! | `LoopStage::MemoryCommit` | `MemoryPort::extract_and_commit` on the text path. |
//! | `LoopStage::StateCommit` | Checkpoint after a branch mutates durable state. |
//! | `LoopStage::Finalize` | `GraphRuntime::complete_run`. |

pub use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
pub use crate::engine_v2::run_agent_loop;
