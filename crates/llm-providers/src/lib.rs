//! LLM Providers - General LLM provider abstraction layer
//!
//! Provides a unified interface for different LLM providers (rig, openai, anthropic, etc.)

mod config;
mod error;
mod message;
mod traits;

// Provider implementations
pub mod rig;

pub use config::{ProviderConfig, ProviderType};
pub use error::{ProviderError, ProviderResult};
pub use message::{Message, Role};
pub use traits::{create_provider, ChatAgent, CompletionClient, LLMProvider};
