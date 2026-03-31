//! MCP Bridge - Enhanced MCP integration for Open Harness
//!
//! 提供技能 MCP 管理、动态 MCP 服务器加载/卸载等功能。

mod config;
mod error;
mod manager;
mod skill_mcp;
mod types;

pub use config::{McpBridgeConfig, McpServerConfig};
pub use error::{McpBridgeError, McpBridgeResult};
pub use manager::McpBridgeManager;
pub use skill_mcp::SkillMcpManager;
pub use types::{RiskLevel, SideEffectClass, ToolManifest, ToolProviderType};
