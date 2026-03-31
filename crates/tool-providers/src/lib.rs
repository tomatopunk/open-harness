//! Tool Providers for Harness
//!
//! This crate provides unified tool provider abstraction supporting:
//! - MCP (Model Context Protocol) tools
//! - Skills (Markdown-based workflow definitions)
//! - Local tools (Rust implementations)
//! - ACP (Agent Client Protocol) agents
//! - Community tools (future extension)

pub mod acp_provider;
pub mod discovery;
pub mod local_provider;
pub mod mcp_provider;
pub mod skill_provider;

pub use discovery::{load_mcp_providers, load_skill_provider, ToolProviderDiscovery};
pub use local_provider::LocalToolProvider;
pub use mcp_provider::McpToolProvider;
pub use skill_provider::SkillToolProvider;

// Re-export ACP provider only if needed (it's optional based on config)
pub use acp_provider::{AcpToolProvider, AcpToolProviderBuilder};
