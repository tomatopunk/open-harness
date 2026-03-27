use async_trait::async_trait;
use protocol_compat::Configurable;
use serde_json::Value;

use crate::types::{RuntimeError, ToolInvocation};

#[derive(Debug, Clone, Default)]
pub struct MiddlewareContext {
    pub configurable: Configurable,
    pub messages: Vec<Value>,
    pub memory_facts: Vec<String>,
    pub todos: Vec<String>,
    pub skill_hints: Vec<String>,
    pub tool_calls: Vec<ToolInvocation>,
    pub loop_detected: bool,
    pub token_usage_estimate: usize,
}

#[async_trait]
pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;
    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError>;
}

pub struct SummarizationMiddleware;
#[async_trait]
impl Middleware for SummarizationMiddleware {
    fn name(&self) -> &'static str {
        "summarization"
    }

    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError> {
        if let Some(last) = ctx.messages.last().and_then(Value::as_str) {
            let summary = last.chars().take(160).collect::<String>();
            ctx.memory_facts.push(format!("summary:{summary}"));
        }
        Ok(())
    }
}

pub struct MemoryMiddleware;
#[async_trait]
impl Middleware for MemoryMiddleware {
    fn name(&self) -> &'static str {
        "memory"
    }

    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError> {
        for msg in &ctx.messages {
            if let Some(s) = msg.as_str() {
                for line in s.lines() {
                    if let Some(fact) = line.strip_prefix("fact:") {
                        let trimmed = fact.trim();
                        if !trimmed.is_empty() && !ctx.memory_facts.iter().any(|x| x == trimmed) {
                            ctx.memory_facts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct TodoMiddleware;
#[async_trait]
impl Middleware for TodoMiddleware {
    fn name(&self) -> &'static str {
        "todo"
    }

    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError> {
        if ctx.configurable.is_plan_mode.unwrap_or(false) && ctx.todos.is_empty() {
            ctx.todos.push("拆分子任务并跟踪状态".to_string());
            ctx.todos.push("执行并回收每个子任务结果".to_string());
        }
        ctx.token_usage_estimate =
            ctx.messages.iter().filter_map(Value::as_str).map(str::len).sum::<usize>();
        ctx.loop_detected = ctx.messages.windows(2).any(|w| w[0] == w[1]);
        Ok(())
    }
}

pub struct ToolDetectMiddleware;
#[async_trait]
impl Middleware for ToolDetectMiddleware {
    fn name(&self) -> &'static str {
        "tool_detect"
    }

    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError> {
        for msg in &ctx.messages {
            if let Some(s) = msg.as_str() {
                if s.contains("search") {
                    ctx.tool_calls.push(ToolInvocation {
                        tool_name: "web_search".to_string(),
                        args: serde_json::json!({"query": s}),
                    });
                }
                if s.contains("read") || s.contains("file") {
                    ctx.tool_calls.push(ToolInvocation {
                        tool_name: "read_file".to_string(),
                        args: serde_json::json!({"hint": s}),
                    });
                }
            }
        }
        Ok(())
    }
}

pub struct ClarificationMiddleware;
#[async_trait]
impl Middleware for ClarificationMiddleware {
    fn name(&self) -> &'static str {
        "clarification"
    }

    async fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), RuntimeError> {
        if ctx.messages.is_empty() {
            return Err(RuntimeError::InvalidInput("messages is empty".to_string()));
        }
        Ok(())
    }
}
