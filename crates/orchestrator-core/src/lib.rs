//! Pure Rust lead-agent style orchestration: middleware chain + state step.

pub mod error;
pub mod middleware;
pub mod pipeline;

pub use error::OrchestratorError;
pub use pipeline::LeadPipeline;
