use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEnvelope {
    pub platform: String,
    pub event_id: String,
    pub user_id: String,
    pub chat_id: String,
    pub message_id: String,
    pub text: Option<String>,
    pub attachments: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedCommand {
    pub command: String,
    pub args: Vec<String>,
    pub thread_hint: Option<String>,
}

#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("unauthorized signature")]
    Unauthorized,
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("upstream error: {0}")]
    Upstream(String),
}

#[async_trait]
pub trait ChannelDriver: Send + Sync {
    fn platform(&self) -> &'static str;
    async fn verify_signature(
        &self,
        headers: &HashMap<String, String>,
        raw_body: &[u8],
    ) -> Result<(), ChannelError>;
    async fn parse_event(&self, raw_body: &[u8]) -> Result<ChannelEnvelope, ChannelError>;
    async fn normalize_command(
        &self,
        env: &ChannelEnvelope,
    ) -> Result<NormalizedCommand, ChannelError>;
    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), ChannelError>;
}
