//! Tool Providers for Harness
//!
//! This crate provides unified tool provider abstraction supporting:
//! - MCP (Model Context Protocol) tools
//! - Skills (Markdown-based workflow definitions)
//! - Local tools (Rust implementations)
//! - Community tools (future extension)

pub mod discovery;
pub mod local_provider;
pub mod mcp_provider;
pub mod skill_provider;

pub use agent_ports::ToolProviderType;
pub use discovery::{load_mcp_providers, load_skill_provider, ToolProviderDiscovery};
pub use local_provider::LocalToolProvider;
pub use mcp_provider::McpToolProvider;
pub use skill_provider::SkillToolProvider;
