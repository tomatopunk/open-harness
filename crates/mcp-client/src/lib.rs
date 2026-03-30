//! MCP (Model Context Protocol) Client for Harness
//!
//! This crate provides MCP client implementation supporting:
//! - HTTP/SSE transport
//! - OAuth 2.0 authentication
//! - Tool discovery and invocation
//! - Configuration-driven server management
//! - Lazy loading with cache invalidation

pub mod cache;
pub mod client;
pub mod oauth;
pub mod types;

pub use cache::{load_mcp_servers_from_file, McpToolsCache};
pub use client::{McpClient, MultiServerMcpClient};
pub use oauth::OAuthTokenManager;
pub use types::*;

/// Re-export commonly used types
pub use serde_json::Value;
