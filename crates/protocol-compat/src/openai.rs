use serde::{Deserialize, Serialize};

use crate::Configurable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<OpenAiChatMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(default)]
    pub response_format: Option<serde_json::Value>,
    /// Optional passthrough runtime knobs for LangGraph configurable.
    #[serde(default)]
    pub configurable: Option<Configurable>,
    /// Optional stream mode override for upstream LangGraph runs/stream.
    #[serde(default)]
    pub stream_mode: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiModelsResponse {
    pub object: String,
    pub data: Vec<OpenAiModelItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiModelItem {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}
