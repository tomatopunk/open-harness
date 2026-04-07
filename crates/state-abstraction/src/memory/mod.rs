//! Memory system module.
//!
//! This module contains all memory-related components:
//! - Configuration
//! - Prompts
//! - Constants
//! - Document management
//! - Voting and conflict resolution
//! - Extraction and merging

pub mod config;
pub mod constants;
pub mod prompts;

pub use config::{
    ConfidenceLevels, ConflictDetectionConfig, MemoryConfig, PromptConfig, VotingConfig,
};
pub use constants::*;
pub use prompts::{MEMORY_UPDATE_PROMPT, MERGE_PROFILE_PROMPT};
