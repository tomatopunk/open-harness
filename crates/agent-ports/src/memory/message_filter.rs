//! Message filter for extracting relevant conversation turns (DeerFlow-inspired).

use crate::thread_state::ChatMessage;

/// Filtered conversation result.
#[derive(Debug, Clone, Default)]
pub struct FilteredConversation {
    /// User messages only.
    pub user_messages: Vec<ChatMessage>,
    /// Final assistant response (if any).
    pub assistant_response: Option<ChatMessage>,
    /// All messages in original order (after filtering).
    pub all_messages: Vec<ChatMessage>,
}

/// Filter conversation messages to extract only relevant parts for memory processing.
///
/// Strategy (DeerFlow-inspired):
/// - Keep all user messages (role="user")
/// - Keep only the final assistant response (last role="assistant" without tool_calls)
/// - Remove intermediate tool calls and results
pub struct MessageFilter;

impl MessageFilter {
    /// Create a new message filter.
    pub const fn new() -> Self {
        Self
    }

    /// Filter a conversation to extract relevant parts.
    #[must_use]
    pub fn filter(&self, messages: &[ChatMessage]) -> FilteredConversation {
        let mut result = FilteredConversation::default();

        // Collect user messages
        result.user_messages = messages.iter().filter(|m| m.role == "user").cloned().collect();

        // Find final assistant response (last one without tool_calls)
        result.assistant_response = messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant" && !self.has_tool_calls(m))
            .cloned();

        // Build filtered message list (user messages + final assistant response)
        result.all_messages = result.user_messages.clone();
        if let Some(assistant_msg) = &result.assistant_response {
            result.all_messages.push(assistant_msg.clone());
        }

        result
    }

    /// Check if a message contains tool calls.
    fn has_tool_calls(&self, message: &ChatMessage) -> bool {
        // Check for tool_calls in message content or metadata
        if let Some(content) = message.content.as_str() {
            // Simple heuristic: check for tool call markers
            content.contains("tool_call") || content.contains("function_call")
        } else if let Some(obj) = message.content.as_object() {
            // Check for structured tool call format
            obj.contains_key("tool_calls") || obj.contains_key("function_call")
        } else {
            false
        }
    }

    /// Format filtered conversation as a string for LLM processing.
    #[must_use]
    pub fn format_for_llm(&self, filtered: &FilteredConversation) -> String {
        let mut parts = Vec::new();

        // Add user messages
        for msg in &filtered.user_messages {
            if let Some(content) = msg.content.as_str() {
                parts.push(format!("User: {}", content));
            } else {
                parts.push(format!("User: {}", msg.content));
            }
        }

        // Add assistant response
        if let Some(assistant_msg) = &filtered.assistant_response {
            if let Some(content) = assistant_msg.content.as_str() {
                parts.push(format!("Assistant: {}", content));
            } else {
                parts.push(format!("Assistant: {}", assistant_msg.content));
            }
        }

        parts.join("\n")
    }

    /// Check if a message is a tool result (to be filtered out).
    #[must_use]
    pub fn is_tool_result(&self, message: &ChatMessage) -> bool {
        message.role == "tool"
            || message.role == "function"
            || (message.role == "assistant" && self.has_tool_calls(message))
    }
}

impl Default for MessageFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_filter_simple_conversation() {
        let filter = MessageFilter::new();
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: json!("Hello") },
            ChatMessage { role: "assistant".to_string(), content: json!("Hi there!") },
            ChatMessage { role: "user".to_string(), content: json!("How are you?") },
            ChatMessage {
                role: "assistant".to_string(),
                content: json!("I'm doing well, thanks!"),
            },
        ];

        let filtered = filter.filter(&messages);

        assert_eq!(filtered.user_messages.len(), 2);
        assert!(filtered.assistant_response.is_some());
        assert_eq!(filtered.all_messages.len(), 3); // 2 user + 1 assistant
        assert_eq!(filtered.assistant_response.unwrap().content, json!("I'm doing well, thanks!"));
    }

    #[test]
    fn test_filter_with_tool_calls() {
        let filter = MessageFilter::new();
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: json!("What's the weather?") },
            ChatMessage {
                role: "assistant".to_string(),
                content: json!({"tool_calls": [{"name": "weather", "args": {}}]}),
            },
            ChatMessage { role: "tool".to_string(), content: json!({"temperature": 25}) },
            ChatMessage {
                role: "assistant".to_string(),
                content: json!("The temperature is 25°C."),
            },
        ];

        let filtered = filter.filter(&messages);

        assert_eq!(filtered.user_messages.len(), 1);
        assert!(filtered.assistant_response.is_some());
        // Should skip the tool call and only keep final response
        assert_eq!(filtered.assistant_response.unwrap().content, json!("The temperature is 25°C."));
    }

    #[test]
    fn test_filter_only_user_messages() {
        let filter = MessageFilter::new();
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: json!("First") },
            ChatMessage { role: "user".to_string(), content: json!("Second") },
        ];

        let filtered = filter.filter(&messages);

        assert_eq!(filtered.user_messages.len(), 2);
        assert!(filtered.assistant_response.is_none());
        assert_eq!(filtered.all_messages.len(), 2);
    }

    #[test]
    fn test_format_for_llm() {
        let filter = MessageFilter::new();
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: json!("Hello") },
            ChatMessage { role: "assistant".to_string(), content: json!("Hi!") },
        ];

        let filtered = filter.filter(&messages);
        let formatted = filter.format_for_llm(&filtered);

        assert!(formatted.contains("User: Hello"));
        assert!(formatted.contains("Assistant: Hi!"));
    }

    #[test]
    fn test_is_tool_result() {
        let filter = MessageFilter::new();

        let tool_msg = ChatMessage { role: "tool".to_string(), content: json!({"result": "ok"}) };
        assert!(filter.is_tool_result(&tool_msg));

        let function_msg =
            ChatMessage { role: "function".to_string(), content: json!({"output": "test"}) };
        assert!(filter.is_tool_result(&function_msg));

        let user_msg = ChatMessage { role: "user".to_string(), content: json!("Hello") };
        assert!(!filter.is_tool_result(&user_msg));
    }
}
