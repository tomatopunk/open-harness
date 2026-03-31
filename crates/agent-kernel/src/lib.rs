//! Open Harness Agent Kernel - 最小化 agent 内核
//!
//! 提供核心的 agent 循环、事件总线和插件管理功能。

mod config;
mod error;
mod events;
mod kernel;
mod lifecycle;

pub use config::KernelConfig;
pub use error::{KernelError, KernelResult};
pub use events::{Event, EventBus, EventHandler};
pub use kernel::AgentKernel;
pub use lifecycle::LifecycleHook;
