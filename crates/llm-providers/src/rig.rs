//! Rig Provider - Implementation using the rig library
//!
//! This provider uses the rig library under the hood.

use crate::config::{ProviderConfig, ProviderType};
use crate::error::ProviderResult;
use crate::message::Message;
use crate::traits::{ChatAgent, CompletionClient, LLMProvider};
use async_trait::async_trait;

/// Rig provider implementation
pub struct RigProvider {
    config: ProviderConfig,
}

impl RigProvider {
    /// Create a new Rig provider
    pub fn new(config: &ProviderConfig) -> ProviderResult<Self> {
        Ok(Self { config: config.clone() })
    }
}

#[async_trait]
impl LLMProvider for RigProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Rig
    }

    fn completion_client(&self) -> Box<dyn CompletionClient> {
        Box::new(RigCompletionClient::new(&self.config))
    }

    fn chat_agent(&self, config: &ProviderConfig) -> ProviderResult<Box<dyn ChatAgent>> {
        Ok(Box::new(RigChatAgent::new(config)?))
    }
}

/// Rig completion client
struct RigCompletionClient {
    #[allow(dead_code)]
    config: ProviderConfig,
}

impl RigCompletionClient {
    fn new(config: &ProviderConfig) -> Self {
        Self { config: config.clone() }
    }
}

#[async_trait]
impl CompletionClient for RigCompletionClient {
    async fn complete(&self, prompt: &str) -> ProviderResult<String> {
        Ok(format!("[Rig] Completion for: {}", prompt))
    }

    async fn complete_stream(
        &self,
        prompt: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let prompt_owned = prompt.to_string();

        tokio::spawn(async move {
            let _ = tx.send(format!("[Rig] Stream: {}", prompt_owned)).await;
        });

        Ok(rx)
    }
}

/// Rig chat agent
struct RigChatAgent {
    config: ProviderConfig,
    preamble: Option<String>,
    tools: Vec<serde_json::Value>,
}

impl RigChatAgent {
    fn new(config: &ProviderConfig) -> ProviderResult<Self> {
        Ok(Self { config: config.clone(), preamble: None, tools: Vec::new() })
    }
}

#[async_trait]
impl ChatAgent for RigChatAgent {
    fn model(&self) -> &str {
        &self.config.model
    }

    fn set_preamble(&mut self, preamble: String) {
        self.preamble = Some(preamble);
    }

    async fn prompt(&mut self, message: &str) -> ProviderResult<String> {
        Ok(format!("[Rig Agent] Response to: {}", message))
    }

    async fn chat(&mut self, _messages: &[Message]) -> ProviderResult<Message> {
        Ok(Message::assistant("[Rig Agent] Chat response"))
    }

    async fn prompt_stream(
        &mut self,
        message: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>> {
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let message_owned = message.to_string();

        tokio::spawn(async move {
            let _ = tx.send(format!("[Rig Agent] Stream response: {}", message_owned)).await;
        });

        Ok(rx)
    }

    fn add_tools(&mut self, tools: Vec<serde_json::Value>) {
        self.tools.extend(tools);
    }

    fn clear_tools(&mut self) {
        self.tools.clear();
    }
}
