use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Provider type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    #[default]
    Rig,
    OpenAI,
    Anthropic,
    Azure,
    Google,
    Bedrock,
}

/// Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    /// Provider type
    pub provider_type: ProviderType,

    /// Model name
    pub model: String,

    /// API key (if required)
    #[serde(default)]
    pub api_key: Option<String>,

    /// Base URL (for custom endpoints)
    #[serde(default)]
    pub base_url: Option<String>,

    /// API version (if required)
    #[serde(default)]
    pub api_version: Option<String>,

    /// Additional provider-specific configuration
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,

    /// Generation parameters
    #[serde(default)]
    pub generation: GenerationParams,
}

/// Generation parameters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationParams {
    /// Temperature (0.0 - 2.0)
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Top-p sampling (0.0 - 1.0)
    #[serde(default)]
    pub top_p: Option<f32>,

    /// Top-k sampling
    #[serde(default)]
    pub top_k: Option<u32>,

    /// Max tokens to generate
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Stop sequences
    #[serde(default)]
    pub stop: Option<Vec<String>>,

    /// Presence penalty
    #[serde(default)]
    pub presence_penalty: Option<f32>,

    /// Frequency penalty
    #[serde(default)]
    pub frequency_penalty: Option<f32>,
}

impl ProviderConfig {
    /// Create a new provider configuration
    pub fn new(provider_type: ProviderType, model: impl Into<String>) -> Self {
        Self {
            provider_type,
            model: model.into(),
            api_key: None,
            base_url: None,
            api_version: None,
            extra: HashMap::new(),
            generation: GenerationParams::default(),
        }
    }

    /// Set API key
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set base URL
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Set generation parameters
    pub fn with_generation(mut self, params: GenerationParams) -> Self {
        self.generation = params;
        self
    }
}
