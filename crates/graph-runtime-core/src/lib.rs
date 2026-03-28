//! Embeddable graph-style runtime: threads, runs, step checkpoints, resume.
//!
//! Persistence is provided by [`agent_ports::CheckpointPort`] (e.g. in-memory adapter or
//! `state_abstraction::DynCheckpointStorePort` over local FS).

pub mod checkpoint_store;
pub mod engine;
pub mod error;

pub use engine::{parse_thread_id, GraphRuntime, RunHandle, RunStatus};
pub use error::{GraphResult, GraphRuntimeError};
