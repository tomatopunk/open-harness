//! Sandbox execution abstraction (Docker via bollard optional).

pub mod error;
pub mod traits;

pub use error::SandboxError;
pub use traits::{LocalSandbox, Sandbox, SandboxOutput, SandboxRequest};
