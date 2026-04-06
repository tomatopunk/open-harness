//! Open Harness Agent Kernel - 最小化 agent 内核
//!
//! 提供核心的 agent 循环、事件总线和插件管理功能。

#![allow(dead_code)]
#![allow(unused_variables)]

mod agent_loop;
mod channel_manager;
mod config;
mod error;
mod events;
mod hooks;
mod kernel;
mod lifecycle;

pub use agent_loop::AgentLoop;
pub use channel_manager::{Channel, ChannelManager, InboundMessage, OutboundMessage};
pub use config::{
    AgentLoopConfig, ChannelsConfig, DingtalkConfig, KernelConfig, KernelConfigResolution,
    KernelMigrationConfig, KernelRuntimeMode, MemoryConfig, ResolvedKernelConfig, StorageConfig,
    StorageMode,
};
pub use error::{KernelError, KernelErrorCategory, KernelResult};
pub use events::{Event, EventBus, EventHandler};
pub use hooks::{HookFn, HookPhase, HookSystem};
pub use kernel::AgentKernel;
pub use lifecycle::LifecycleHook;
pub use llm_providers::{ProviderConfig, ProviderType};
