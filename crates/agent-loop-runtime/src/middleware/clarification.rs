//! Clarification Middleware - intercepts clarification requests and presents them to the user
//!
//! This middleware intercepts `ask_clarification` tool calls and converts them into
//! user-facing interrupt messages, similar to DeerFlow's ClarificationMiddleware.

use crate::error::AgentLoopResult;
use crate::middleware::{AgentLoopMiddleware, TurnContext};
use agent_ports::{LlmTurnOutput, ThreadState, ToolCallSpec};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Clarification type enum for categorizing clarification requests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClarificationType {
    /// Missing information needed to proceed
    #[default]
    MissingInfo,
    /// Ambiguous requirement that needs clarification
    AmbiguousRequirement,
    /// Choice between different approaches
    ApproachChoice,
    /// Risk confirmation needed
    RiskConfirmation,
    /// Suggestion for user consideration
    Suggestion,
}

/// Arguments for ask_clarification tool
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClarificationRequest {
    /// The clarification question to ask the user
    pub question: String,

    /// Type of clarification needed
    #[serde(default)]
    pub clarification_type: ClarificationType,

    /// Optional context explaining why clarification is needed
    #[serde(default)]
    pub context: Option<String>,

    /// Optional list of suggested options/choices
    #[serde(default)]
    pub options: Vec<String>,
}

/// Clarification middleware that intercepts and formats clarification requests
pub struct ClarificationMiddleware {
    /// Whether to enable the middleware
    enabled: bool,
}

impl ClarificationMiddleware {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check if a tool call is a clarification request
    fn is_clarification_call(call: &ToolCallSpec) -> bool {
        call.name == "ask_clarification"
    }

    /// Format clarification arguments into a user-friendly message
    fn format_clarification_message(args: &Value) -> String {
        let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("Please clarify");

        let clarification_type =
            args.get("clarification_type").and_then(|v| v.as_str()).unwrap_or("missing_info");

        let context = args.get("context").and_then(|v| v.as_str());

        let options = args
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Type-specific icons
        let icon = match clarification_type {
            "missing_info" => "❓",
            "ambiguous_requirement" => "🤔",
            "approach_choice" => "🔀",
            "risk_confirmation" => "⚠️",
            "suggestion" => "💡",
            _ => "❓",
        };

        let mut message_parts = Vec::new();

        // Add icon and question together for a natural flow
        if let Some(ctx) = context {
            // If there's context, present it first as background
            message_parts.push(format!("{} {}", icon, ctx));
            message_parts.push(question.to_string());
        } else {
            // Just the question with icon
            message_parts.push(format!("{} {}", icon, question));
        }

        // Add options in a cleaner format
        if !options.is_empty() {
            message_parts.push(String::new()); // blank line for spacing
            for (i, option) in options.iter().enumerate() {
                message_parts.push(format!("  {}. {}", i + 1, option));
            }
        }

        message_parts.join("\n")
    }

    /// Handle clarification request by formatting the message
    fn handle_clarification(&self, call: &ToolCallSpec) -> AgentLoopResult<()> {
        let args = &call.args;
        let question = args.get("question").and_then(|v| v.as_str()).unwrap_or("Unknown");

        tracing::debug!(
            target: "clarification",
            "Intercepted clarification request: {}",
            question
        );

        // Format the clarification message for logging
        let formatted_message = Self::format_clarification_message(args);
        tracing::info!(
            target: "clarification",
            "Formatted clarification:\n{}",
            formatted_message
        );

        // The actual interrupt handling is done by the engine
        // This middleware just provides the formatting and logging
        Ok(())
    }
}

#[async_trait]
impl AgentLoopMiddleware for ClarificationMiddleware {
    async fn before_model(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _messages_for_llm: &mut Vec<Value>,
    ) -> AgentLoopResult<()> {
        // No-op: clarification is handled at tool call time
        Ok(())
    }

    async fn after_model(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        out: &LlmTurnOutput,
    ) -> AgentLoopResult<()> {
        // Check if model output contains clarification tool calls
        for call in &out.tool_calls {
            if Self::is_clarification_call(call) {
                self.handle_clarification(call)?;
            }
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        call: &ToolCallSpec,
    ) -> AgentLoopResult<()> {
        // Intercept clarification tool calls before execution
        if self.enabled && Self::is_clarification_call(call) {
            self.handle_clarification(call)?;
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _call: &ToolCallSpec,
        _ok: bool,
        _payload: &Value,
    ) -> AgentLoopResult<()> {
        // No-op after tool execution
        Ok(())
    }
}

/// Builder for ClarificationMiddleware
pub struct ClarificationMiddlewareBuilder {
    enabled: bool,
}

impl ClarificationMiddlewareBuilder {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn build(self) -> Arc<ClarificationMiddleware> {
        Arc::new(ClarificationMiddleware::new(self.enabled))
    }
}

impl Default for ClarificationMiddlewareBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_clarification_simple() {
        let args = json!({
            "question": "What is your preferred approach?",
            "clarification_type": "missing_info"
        });

        let message = ClarificationMiddleware::format_clarification_message(&args);
        assert!(message.contains("❓"));
        assert!(message.contains("What is your preferred approach?"));
    }

    #[test]
    fn test_format_clarification_with_context() {
        let args = json!({
            "question": "Which database should we use?",
            "clarification_type": "approach_choice",
            "context": "I need to know your database preference to proceed."
        });

        let message = ClarificationMiddleware::format_clarification_message(&args);
        assert!(message.contains("🔀"));
        assert!(message.contains("I need to know your database preference"));
        assert!(message.contains("Which database should we use?"));
    }

    #[test]
    fn test_format_clarification_with_options() {
        let args = json!({
            "question": "Choose a deployment strategy",
            "clarification_type": "approach_choice",
            "options": ["Blue-Green", "Canary", "Rolling Update"]
        });

        let message = ClarificationMiddleware::format_clarification_message(&args);
        assert!(message.contains("🔀"));
        assert!(message.contains("1. Blue-Green"));
        assert!(message.contains("2. Canary"));
        assert!(message.contains("3. Rolling Update"));
    }

    #[test]
    fn test_is_clarification_call() {
        let clar_call = ToolCallSpec {
            name: "ask_clarification".to_string(),
            args: json!({"question": "test"}),
            call_id: "1".to_string(),
        };

        let other_call = ToolCallSpec {
            name: "read_file".to_string(),
            args: json!({"path": "test.txt"}),
            call_id: "2".to_string(),
        };

        assert!(ClarificationMiddleware::is_clarification_call(&clar_call));
        assert!(!ClarificationMiddleware::is_clarification_call(&other_call));
    }
}
