//! Heuristic LLM port for tests and offline demos (no external API).

use std::sync::Arc;

use agent_ports::{
    LLMPort, LlmTurnContext, LlmTurnOutput, PortResult, SubtaskPlan, SubtaskSpec, ToolCallSpec,
};
use async_trait::async_trait;
use serde_json::Value;

/// Parses user messages for keywords to synthesize tool calls or final text.
#[derive(Debug, Clone, Default)]
pub struct HeuristicLlmAdapter {
    pub model_hint: Option<String>,
}

#[async_trait]
impl LLMPort for HeuristicLlmAdapter {
    async fn infer_turn(&self, ctx: LlmTurnContext) -> PortResult<LlmTurnOutput> {
        let last = ctx
            .messages
            .iter()
            .filter_map(|v| v.get("content"))
            .filter_map(Value::as_str)
            .next_back()
            .unwrap_or("");

        if last.contains("subagent:") {
            return Ok(LlmTurnOutput {
                assistant_text: None,
                tool_calls: vec![],
                subtask_plan: Some(SubtaskPlan {
                    tasks: vec![SubtaskSpec {
                        goal: last.replace("subagent:", "").trim().to_string(),
                        input: Value::Null,
                        budget_steps: 4,
                    }],
                }),
                needs_clarification: false,
                clarification_prompt: None,
                finish_turn: false,
            });
        }

        if last.contains("tool:") {
            let name = last
                .split("tool:")
                .nth(1)
                .unwrap_or("echo")
                .split_whitespace()
                .next()
                .unwrap_or("echo")
                .to_string();
            return Ok(LlmTurnOutput {
                assistant_text: None,
                tool_calls: vec![ToolCallSpec {
                    name,
                    args: Value::Object(serde_json::Map::new()),
                    call_id: format!("call-{}", uuid::Uuid::new_v4()),
                }],
                subtask_plan: None,
                needs_clarification: false,
                clarification_prompt: None,
                finish_turn: true,
            });
        }

        if last.contains("clarify") {
            return Ok(LlmTurnOutput {
                assistant_text: None,
                tool_calls: vec![],
                subtask_plan: None,
                needs_clarification: true,
                clarification_prompt: Some("Please specify your goal.".into()),
                finish_turn: true,
            });
        }

        let model = ctx.model_name.as_deref().or(self.model_hint.as_deref()).unwrap_or("heuristic");
        let mut text = format!("echo:{model}");
        if ctx.is_plan_mode {
            text.push_str(" | plan_mode");
        }
        if !ctx.assembled_tool_names.is_empty() {
            text.push_str(" | tools:[");
            text.push_str(&ctx.assembled_tool_names.join(","));
            text.push(']');
        }
        Ok(LlmTurnOutput {
            assistant_text: Some(text),
            tool_calls: vec![],
            subtask_plan: None,
            needs_clarification: false,
            clarification_prompt: None,
            finish_turn: true,
        })
    }
}

/// Wrap any `ChatModel` as a simple single-shot completion LLM port.
pub struct ChatModelLlmAdapter {
    pub inner: Arc<dyn model_runtime::ChatModel + Send + Sync>,
}

#[async_trait]
impl LLMPort for ChatModelLlmAdapter {
    async fn infer_turn(&self, ctx: LlmTurnContext) -> PortResult<LlmTurnOutput> {
        let text = self
            .inner
            .complete(ctx.messages)
            .await
            .map_err(|e| agent_ports::PortError::Llm(e.to_string()))?;
        Ok(LlmTurnOutput {
            assistant_text: Some(text),
            tool_calls: vec![],
            subtask_plan: None,
            needs_clarification: false,
            clarification_prompt: None,
            finish_turn: true,
        })
    }
}
