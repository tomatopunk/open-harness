//! OpenAI Chat Completions as [`LLMPort`] with structured tool calls (function calling).

use agent_ports::{LLMPort, LlmTurnContext, LlmTurnOutput, PortResult, ToolCallSpec};
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionTool, ChatCompletionToolType,
    CreateChatCompletionRequestArgs, FunctionObject,
};
use async_openai::Client;
use async_trait::async_trait;
use serde_json::Value;

fn content_to_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else {
        v.to_string()
    }
}

fn json_messages_to_chat_messages(messages: &[Value]) -> Vec<ChatCompletionRequestMessage> {
    let mut out = Vec::new();
    for m in messages {
        let Some(role) = m.get("role").and_then(|r| r.as_str()) else {
            continue;
        };
        let content_val = m.get("content").cloned().unwrap_or(Value::Null);
        let text = content_to_string(&content_val);
        match role {
            "system" => {
                out.push(ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(text),
                    name: None,
                }))
            }
            "user" => {
                out.push(ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(text),
                    name: None,
                }))
            }
            "assistant" => out.push(ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content: Some(
                        async_openai::types::ChatCompletionRequestAssistantMessageContent::Text(
                            text,
                        ),
                    ),
                    ..Default::default()
                },
            )),
            "tool" => {
                let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) else {
                    continue;
                };
                out.push(ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                    content: ChatCompletionRequestToolMessageContent::Text(text),
                    tool_call_id: id.to_string(),
                }));
            }
            _ => {}
        }
    }
    out
}

/// Real LLM turn using OpenAI Chat Completions + optional function tools.
pub struct OpenAiChatLlmAdapter {
    pub model: String,
    client: Client<async_openai::config::OpenAIConfig>,
}

impl OpenAiChatLlmAdapter {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self { model: model.into(), client: Client::new() }
    }
}

#[async_trait]
impl LLMPort for OpenAiChatLlmAdapter {
    async fn infer_turn(&self, ctx: LlmTurnContext) -> PortResult<LlmTurnOutput> {
        let mut messages = json_messages_to_chat_messages(&ctx.messages);
        if let Some(prompt) = ctx.system_prompt.filter(|s| !s.is_empty()) {
            messages.insert(
                0,
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(prompt),
                    name: None,
                }),
            );
        }

        let model_id =
            ctx.model_name.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(&self.model);

        let mut req = CreateChatCompletionRequestArgs::default();
        req.model(model_id).messages(messages);

        if !ctx.assembled_tool_names.is_empty() {
            let tools: Vec<ChatCompletionTool> = ctx
                .assembled_tool_names
                .iter()
                .map(|name| ChatCompletionTool {
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionObject {
                        name: name.clone(),
                        description: Some(format!("Invoke tool `{name}`")),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "additionalProperties": true
                        })),
                        strict: None,
                    },
                })
                .collect();
            req.tools(tools);
        }

        let request = req.build().map_err(|e| agent_ports::PortError::Llm(e.to_string()))?;
        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| agent_ports::PortError::Llm(e.to_string()))?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| agent_ports::PortError::Llm("empty completion choices".into()))?;
        let msg = choice.message;

        if let Some(tool_calls) = msg.tool_calls {
            let mut specs = Vec::new();
            for ChatCompletionMessageToolCall { id, function, .. } in tool_calls {
                let args: Value = serde_json::from_str(&function.arguments)
                    .unwrap_or_else(|_| serde_json::json!({ "raw": function.arguments }));
                specs.push(ToolCallSpec { name: function.name, args, call_id: id });
            }
            return Ok(LlmTurnOutput {
                assistant_text: msg.content,
                tool_calls: specs,
                subtask_plan: None,
                needs_clarification: false,
                clarification_prompt: None,
                finish_turn: false,
            });
        }

        Ok(LlmTurnOutput {
            assistant_text: msg.content,
            tool_calls: vec![],
            subtask_plan: None,
            needs_clarification: false,
            clarification_prompt: None,
            finish_turn: true,
        })
    }
}
