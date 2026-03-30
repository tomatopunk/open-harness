//! Immutable constants for memory system.
//!
//! These are compile-time constants that should not be changed by users.

/// Schema version for memory documents.
pub const MEMORY_DOCUMENT_SCHEMA_VERSION: u32 = 2;

/// Default characters per token for token counting.
pub const DEFAULT_CHARS_PER_TOKEN: usize = 4;

/// Maximum retry attempts for LLM calls.
pub const MAX_LLM_RETRY_ATTEMPTS: usize = 3;

/// Default timeout for LLM calls in seconds.
pub const DEFAULT_LLM_TIMEOUT_SECS: u64 = 30;
