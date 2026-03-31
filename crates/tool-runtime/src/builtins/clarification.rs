//! Built-in clarification tool for requesting user input
//!
//! This tool allows the agent to request clarification from the user when needed.

use crate::{registry::Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

/// Clarification tool implementation
pub struct AskClarificationTool {
    /// Optional callback for handling clarification requests
    handler: Option<Arc<dyn ClarificationHandler>>,
}

/// Handler trait for processing clarification requests
#[async_trait::async_trait]
pub trait ClarificationHandler: Send + Sync {
    /// Called when a clarification request is made
    async fn handle_clarification(
        &self,
        request: &ClarificationRequest,
    ) -> Result<Value, ToolError>;
}

/// Clarification request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationRequest {
    /// The clarification question
    pub question: String,
    /// Type of clarification needed
    #[serde(default)]
    pub clarification_type: String,
    /// Optional context
    #[serde(default)]
    pub context: Option<String>,
    /// Optional suggested options
    #[serde(default)]
    pub options: Vec<String>,
}

impl AskClarificationTool {
    pub fn new(handler: Option<Arc<dyn ClarificationHandler>>) -> Self {
        Self { handler }
    }

    /// Create a new instance with no handler (uses default interrupt behavior)
    pub fn standalone() -> Self {
        Self { handler: None }
    }
}

#[async_trait::async_trait]
impl Tool for AskClarificationTool {
    fn name(&self) -> &'static str {
        "ask_clarification"
    }

    async fn invoke(&self, args: Value) -> Result<Value, ToolError> {
        info!("ask_clarification tool invoked with args: {:?}", args);

        // Parse the arguments
        let request = ClarificationRequest {
            question: args
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Please clarify")
                .to_string(),
            clarification_type: args
                .get("clarification_type")
                .and_then(|v| v.as_str())
                .unwrap_or("missing_info")
                .to_string(),
            context: args.get("context").and_then(|v| v.as_str()).map(String::from),
            options: args
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(String::from).collect())
                .unwrap_or_default(),
        };

        // If a handler is provided, use it
        if let Some(ref handler) = self.handler {
            handler.handle_clarification(&request).await
        } else {
            // Default behavior: return the clarification request as-is
            // The middleware will intercept this and convert it to an interrupt
            Ok(serde_json::json!({
                "type": "clarification_request",
                "question": request.question,
                "clarification_type": request.clarification_type,
                "context": request.context,
                "options": request.options,
                "status": "pending_user_response"
            }))
        }
    }
}

/// Create a ToolManifest for ask_clarification tool
pub fn ask_clarification_manifest() -> agent_ports::ToolManifest {
    use agent_ports::{RiskLevel, SideEffectClass, ToolProviderType};

    agent_ports::ToolManifest {
        name: "ask_clarification".to_string(),
        description: Some(
            "Request clarification from the user when more information is needed to proceed"
                .to_string(),
        ),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The clarification question to ask the user"
                },
                "clarification_type": {
                    "type": "string",
                    "description": "Type of clarification: missing_info, ambiguous_requirement, approach_choice, risk_confirmation, or suggestion",
                    "enum": ["missing_info", "ambiguous_requirement", "approach_choice", "risk_confirmation", "suggestion"]
                },
                "context": {
                    "type": "string",
                    "description": "Optional context explaining why clarification is needed"
                },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    },
                    "description": "Optional list of suggested options or choices"
                }
            },
            "required": ["question"]
        })),
        capability_tags: vec!["builtin".to_string(), "clarification".to_string()],
        risk_level: RiskLevel::Low,
        timeout_ms: 5000,
        retry_max: 0,
        side_effect_class: SideEffectClass::None,
        provider_type: ToolProviderType::Local,
        provider_name: "builtin".to_string(),
        load_path: None,
        version: Some("1.0.0".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_ask_clarification_basic() {
        let tool = AskClarificationTool::standalone();

        let args = json!({
            "question": "What is your preferred approach?",
            "clarification_type": "missing_info"
        });

        let result = tool.invoke(args).await.unwrap();
        assert_eq!(result["type"], "clarification_request");
        assert_eq!(result["question"], "What is your preferred approach?");
        assert_eq!(result["status"], "pending_user_response");
    }

    #[tokio::test]
    async fn test_ask_clarification_with_options() {
        let tool = AskClarificationTool::standalone();

        let args = json!({
            "question": "Choose a deployment strategy",
            "clarification_type": "approach_choice",
            "options": ["Blue-Green", "Canary", "Rolling"]
        });

        let result = tool.invoke(args).await.unwrap();
        assert_eq!(result["options"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_ask_clarification_minimal() {
        let tool = AskClarificationTool::standalone();

        let args = json!({
            "question": "Help?"
        });

        let result = tool.invoke(args).await.unwrap();
        assert_eq!(result["clarification_type"], "missing_info"); // default
        assert!(result["context"].is_null());
        assert_eq!(result["options"].as_array().unwrap().len(), 0);
    }
}
