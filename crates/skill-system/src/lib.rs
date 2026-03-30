//! Skill System for Harness
//!
//! This crate provides skill management functionality:
//! - Skill loading from filesystem
//! - SKILL.md parsing with YAML frontmatter
//! - Skill installation from .skill packages
//! - Skill state management (enabled/disabled)

pub mod installer;
pub mod loader;
pub mod parser;
pub mod types;

pub use installer::SkillInstaller;
pub use loader::SkillLoader;
pub use parser::parse_skill_file;
pub use types::*;
