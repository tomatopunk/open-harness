use crate::config::ProviderConfig;
use crate::message::Message;
use crate::{ProviderError, ProviderResult};
use async_trait::async_trait;

/// Main LLM provider trait
#[async_trait]
pub trait LLMProvider: Send + Sync + 'static {
    /// Provider type
    fn provider_type(&self) -> crate::config::ProviderType;

    /// Create a completion client
    fn completion_client(&self) -> Box<dyn CompletionClient>;

    /// Create a chat agent
    fn chat_agent(&self, config: &ProviderConfig) -> ProviderResult<Box<dyn ChatAgent>>;
}

/// Completion client - for simple text completions
#[async_trait]
pub trait CompletionClient: Send + Sync + 'static {
    /// Complete a prompt
    async fn complete(&self, prompt: &str) -> ProviderResult<String>;

    /// Complete a prompt with streaming
    async fn complete_stream(
        &self,
        prompt: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>>;
}

/// Chat agent - for multi-turn conversations
#[async_trait]
pub trait ChatAgent: Send + Sync + 'static {
    /// Get model name
    fn model(&self) -> &str;

    /// Set system prompt
    fn set_preamble(&mut self, preamble: String);

    /// Send a single message and get response
    async fn prompt(&mut self, message: &str) -> ProviderResult<String>;

    /// Send multiple messages and get response
    async fn chat(&mut self, messages: &[Message]) -> ProviderResult<Message>;

    /// Send a message with streaming response
    async fn prompt_stream(
        &mut self,
        message: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>>;

    /// Add tools to the agent
    fn add_tools(&mut self, tools: Vec<serde_json::Value>);

    /// Clear tools
    fn clear_tools(&mut self);
}

/// Create a provider from configuration
#[allow(dead_code)]
pub fn create_provider(config: &ProviderConfig) -> ProviderResult<Box<dyn LLMProvider>> {
    match config.provider_type {
        crate::config::ProviderType::Rig => Ok(Box::new(crate::rig::RigProvider::new(config)?)),

        crate::config::ProviderType::OpenAI => Err(ProviderError::UnsupportedProvider(
            "OpenAI provider not implemented yet".to_string(),
        )),

        _ => Err(ProviderError::UnsupportedProvider(format!(
            "Provider {:?} not available",
            config.provider_type
        ))),
    }
}
