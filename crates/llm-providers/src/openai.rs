//! OpenAI Provider - Implementation for OpenAI API
//!
//! This provider implements the OpenAI API specification, including:
//! - Chat completions
//! - Text completions
//! - Streaming responses
//! - Tool/function calling

use crate::config::{ProviderConfig, ProviderType};
use crate::error::{ProviderError, ProviderResult};
use crate::message::{Message, Role};
use crate::traits::{ChatAgent, CompletionClient, LLMProvider};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE},
    Client as HttpClient,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// OpenAI provider implementation
pub struct OpenAIProvider {
    config: ProviderConfig,
    client: OpenAIClient,
}

/// Internal OpenAI API client
struct OpenAIClient {
    http_client: HttpClient,
    api_key: String,
    base_url: String,
}

impl fmt::Debug for OpenAIClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAIClient")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl OpenAIProvider {
    /// Create a new OpenAI provider
    pub fn new(config: &ProviderConfig) -> ProviderResult<Self> {
        let api_key = config
            .api_key
            .clone()
            .ok_or_else(|| ProviderError::Config("OpenAI API key is required".to_string()))?;

        let base_url =
            config.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let client = OpenAIClient {
            http_client: HttpClient::new(),
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        };

        Ok(Self { config: config.clone(), client })
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAI
    }

    fn completion_client(&self) -> Box<dyn CompletionClient> {
        Box::new(OpenAICompletionClient {
            config: self.config.clone(),
            client: self.client.clone(),
        })
    }

    fn chat_agent(&self, config: &ProviderConfig) -> ProviderResult<Box<dyn ChatAgent>> {
        Ok(Box::new(OpenAIChatAgent::new(config, self.client.clone())?))
    }
}

/// OpenAI completion client
#[derive(Clone)]
struct OpenAICompletionClient {
    config: ProviderConfig,
    client: OpenAIClient,
}

/// OpenAI completion request
#[derive(Debug, Serialize)]
struct CompletionRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    stream: bool,
}

/// OpenAI completion response
#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<CompletionChoice>,
    error: Option<OpenAIError>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    text: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAIError {
    message: String,
    r#type: String,
}

impl OpenAIClient {
    /// Create headers with authorization
    fn headers(&self) -> ProviderResult<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.api_key)
                .parse()
                .map_err(|e| ProviderError::Config(format!("Invalid API key: {}", e)))?,
        );
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());
        Ok(headers)
    }
}

#[async_trait]
impl CompletionClient for OpenAICompletionClient {
    async fn complete(&self, prompt: &str) -> ProviderResult<String> {
        let request = CompletionRequest {
            model: self.config.model.clone(),
            prompt: prompt.to_string(),
            temperature: self.config.generation.temperature,
            top_p: self.config.generation.top_p,
            max_tokens: self.config.generation.max_tokens,
            stop: self.config.generation.stop.clone(),
            presence_penalty: self.config.generation.presence_penalty,
            frequency_penalty: self.config.generation.frequency_penalty,
            stream: false,
        };

        let url = format!("{}/completions", self.client.base_url);
        let headers = self.client.headers()?;

        let response = self
            .client
            .http_client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| ProviderError::Api(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("API returned {}: {}", status, text)));
        }

        let result: CompletionResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Api(format!("JSON parse failed: {}", e)))?;

        if let Some(error) = result.error {
            return Err(ProviderError::Api(error.message));
        }

        Ok(result.choices.first().map(|c| c.text.clone()).unwrap_or_default())
    }

    async fn complete_stream(
        &self,
        prompt: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>> {
        let request = CompletionRequest {
            model: self.config.model.clone(),
            prompt: prompt.to_string(),
            temperature: self.config.generation.temperature,
            top_p: self.config.generation.top_p,
            max_tokens: self.config.generation.max_tokens,
            stop: self.config.generation.stop.clone(),
            presence_penalty: self.config.generation.presence_penalty,
            frequency_penalty: self.config.generation.frequency_penalty,
            stream: true,
        };

        let url = format!("{}/completions", self.client.base_url);
        let headers = self.client.headers()?;

        let response = self
            .client
            .http_client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| ProviderError::Api(format!("Stream request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("API returned {}: {}", status, text)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let mut stream = response.bytes_stream().eventsource();

        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            break;
                        }
                        match serde_json::from_str::<CompletionResponse>(&event.data) {
                            Ok(parsed) => {
                                if let Some(text) = parsed.choices.first().map(|c| &c.text) {
                                    let _ = tx.send(text.clone()).await;
                                }
                            }
                            Err(_) => {
                                // Ignore parse errors for partial chunks
                            }
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// OpenAI chat agent
struct OpenAIChatAgent {
    config: ProviderConfig,
    client: OpenAIClient,
    preamble: Option<String>,
    tools: Vec<Value>,
}

/// OpenAI chat message
#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// OpenAI tool definition
#[derive(Debug, Serialize, Deserialize)]
struct OpenAITool {
    r#type: String,
    function: Value,
}

/// OpenAI chat completion request
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<OpenAIChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    stream: bool,
}

/// OpenAI chat completion response
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
    error: Option<OpenAIError>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionResponseMessage,
    delta: Option<ChatCompletionResponseMessage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ChatCompletionResponseMessage {
    role: Option<String>,
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    id: String,
    r#type: String,
    function: OpenAIFunctionCall,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

impl OpenAIChatAgent {
    /// Create a new OpenAI chat agent
    fn new(config: &ProviderConfig, client: OpenAIClient) -> ProviderResult<Self> {
        Ok(Self { config: config.clone(), client, preamble: None, tools: Vec::new() })
    }

    /// Convert our Message to OpenAI message
    fn convert_message(msg: &Message) -> OpenAIChatMessage {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        OpenAIChatMessage {
            role: role.to_string(),
            content: msg.content.clone(),
            name: msg.name.clone(),
            tool_call_id: msg.tool_call_id.clone(),
        }
    }

    /// Build chat completion request
    fn build_request(&self, messages: &[Message]) -> ChatCompletionRequest {
        let mut openai_messages = Vec::new();

        // Add system preamble if present
        if let Some(preamble) = &self.preamble {
            openai_messages.push(OpenAIChatMessage {
                role: "system".to_string(),
                content: preamble.clone(),
                name: None,
                tool_call_id: None,
            });
        }

        // Add conversation messages
        openai_messages.extend(messages.iter().map(Self::convert_message));

        // Convert tools to OpenAI format
        let tools = if self.tools.is_empty() {
            None
        } else {
            Some(
                self.tools
                    .iter()
                    .map(|tool| OpenAITool {
                        r#type: "function".to_string(),
                        function: tool.clone(),
                    })
                    .collect(),
            )
        };

        ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: openai_messages,
            temperature: self.config.generation.temperature,
            top_p: self.config.generation.top_p,
            max_tokens: self.config.generation.max_tokens,
            stop: self.config.generation.stop.clone(),
            presence_penalty: self.config.generation.presence_penalty,
            frequency_penalty: self.config.generation.frequency_penalty,
            tools,
            stream: false,
        }
    }

    /// Build chat completion request for streaming
    fn build_stream_request(&self, messages: &[Message]) -> ChatCompletionRequest {
        let mut req = self.build_request(messages);
        req.stream = true;
        req
    }
}

#[async_trait]
impl ChatAgent for OpenAIChatAgent {
    fn model(&self) -> &str {
        &self.config.model
    }

    fn set_preamble(&mut self, preamble: String) {
        self.preamble = Some(preamble);
    }

    async fn prompt(&mut self, message: &str) -> ProviderResult<String> {
        let msg = Message::user(message);
        let response = self.chat(&[msg]).await?;
        Ok(response.content)
    }

    async fn chat(&mut self, messages: &[Message]) -> ProviderResult<Message> {
        let request = self.build_request(messages);
        let url = format!("{}/chat/completions", self.client.base_url);
        let headers = self.client.headers()?;

        let response = self
            .client
            .http_client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| ProviderError::Api(format!("Chat request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("API returned {}: {}", status, text)));
        }

        let result: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Api(format!("JSON parse failed: {}", e)))?;

        if let Some(error) = result.error {
            return Err(ProviderError::Api(error.message));
        }

        let choice = result
            .choices
            .first()
            .ok_or_else(|| ProviderError::Api("No response choices available".to_string()))?;

        let content = choice.message.content.clone().unwrap_or_default();

        // For now, we just return the content as a simple assistant message
        // Tool call handling could be added upstream if needed
        Ok(Message::assistant(content))
    }

    async fn prompt_stream(
        &mut self,
        message: &str,
    ) -> ProviderResult<tokio::sync::mpsc::Receiver<String>> {
        let msg = Message::user(message);
        let request = self.build_stream_request(&[msg]);
        let url = format!("{}/chat/completions", self.client.base_url);
        let headers = self.client.headers()?;

        let response = self
            .client
            .http_client
            .post(&url)
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(|e| ProviderError::Api(format!("Stream chat failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("API returned {}: {}", status, text)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let mut stream = response.bytes_stream().eventsource();

        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if event.data == "[DONE]" {
                            break;
                        }
                        match serde_json::from_str::<ChatCompletionResponse>(&event.data) {
                            Ok(parsed) => {
                                if let Some(choice) = parsed.choices.first() {
                                    if let Some(delta) = &choice.delta {
                                        if let Some(content) = &delta.content {
                                            if !content.is_empty() {
                                                let _ = tx.send(content.clone()).await;
                                            }
                                        }
                                    } else if let Some(content) = &choice.message.content {
                                        let _ = tx.send(content.clone()).await;
                                    }
                                }
                            }
                            Err(_) => {
                                // Ignore parse errors for partial chunks
                            }
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    fn add_tools(&mut self, tools: Vec<Value>) {
        self.tools.extend(tools);
    }

    fn clear_tools(&mut self) {
        self.tools.clear();
    }
}

/// Clone is required because we wrap OpenAIClient in trait objects
/// HttpClient already is Clone
impl Clone for OpenAIClient {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
        }
    }
}
