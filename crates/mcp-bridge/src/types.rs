//! Types for MCP Bridge
//!
//! Core types used across the MCP bridge.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Option<Value>,
    #[serde(default)]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retry_max: u32,
    #[serde(default)]
    pub side_effect_class: SideEffectClass,
    #[serde(default)]
    pub provider_type: ToolProviderType,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub load_path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

/// Risk level for tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

/// Side effect class for tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClass {
    Read,
    Write,
    #[default]
    ReadWrite,
    None,
}

/// Tool provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderType {
    #[default]
    Local,
    Mcp,
    Skill,
    Community,
}
