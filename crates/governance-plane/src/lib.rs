//! Governance plane: YAML-driven policy, tool manifests, and budgets.
//!
//! Services load a directory of YAML files and pass derived structs into the agent loop.

pub mod bundle;
pub mod error;
pub mod models;
pub mod policies;
pub mod skills;
pub mod subagents;
pub mod tools;

pub use bundle::GovernanceBundle;
pub use error::{GovernanceError, GovernanceResult};
pub use skills::{SkillEntry, SkillsFile};
