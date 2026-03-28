//! Embeddable graph-style runtime: threads, runs, step checkpoints, resume.
//!
//! This crate is intentionally persistence-agnostic; the default `MemoryCheckpointStore`
//! is suitable for tests and single-process deployments.

pub mod checkpoint_store;
pub mod engine;
pub mod error;

pub use checkpoint_store::MemoryCheckpointStore;
pub use engine::{parse_thread_id, GraphRuntime, RunHandle, RunStatus};
pub use error::{GraphResult, GraphRuntimeError};
