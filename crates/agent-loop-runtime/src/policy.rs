//! Execution policy extension point (LangChain `RunnableRetry` / `RunnableWithFallbacks` analog).
//!
//! Per-node retry and fallback behavior remains in adapters today; this module reserves a
//! type-level hook for future kernel-level policy injection.

/// Default policy bundle (placeholder for future fields).
#[derive(Debug, Clone, Default)]
pub struct NodePolicyDefaults;
