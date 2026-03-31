//! Skill System for Harness
//!
//! This crate provides skill management functionality:
//! - Skill loading from filesystem
//! - SKILL.md parsing with YAML frontmatter
//! - Skill installation from .skill packages
//! - Skill state management (enabled/disabled)
//! - Skill validation and security checks

pub mod installer;
pub mod loader;
pub mod parser;
pub mod types;
pub mod validation;

pub use installer::SkillInstaller;
pub use loader::SkillLoader;
pub use parser::parse_skill_file;
pub use types::*;
pub use validation::{validate_skill_archive_name, validate_skill_frontmatter};
