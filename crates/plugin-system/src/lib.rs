//! Open Harness Plugin System
//!
//! 提供插件定义、发现、加载和管理功能。

mod error;
mod manager;
mod manifest;
mod plugin;

pub use error::{PluginError, PluginErrorCategory, PluginResult};
pub use manager::{PluginManager, PluginStatus};
pub use manifest::PluginManifest;
pub use plugin::{BasePlugin, Plugin, PluginContext, PluginLifecycleStage, PluginState};
