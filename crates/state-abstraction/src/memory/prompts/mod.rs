//! Prompt templates for LLM-based memory extraction.
//!
//! All prompt templates are loaded at compile time using `include_str!` macro.

/// Prompt template for memory update extraction.
pub const MEMORY_UPDATE_PROMPT: &str = include_str!("update_prompt.txt");

/// Prompt template for merging multiple thread memories.
pub const MERGE_PROFILE_PROMPT: &str = include_str!("merge_prompt.txt");
