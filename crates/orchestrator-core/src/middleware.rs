use protocol_compat::Configurable;
use serde_json::Value;

/// Ordered middleware hooks aligned with deer-flow lead_agent (subset).
#[derive(Debug, Clone, Default)]
pub struct MiddlewareContext {
    pub configurable: Configurable,
    pub messages: Vec<Value>,
    pub memory_facts: Vec<String>,
    pub todos: Vec<String>,
    pub token_usage_estimate: usize,
    pub loop_detected: bool,
}

pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;
    fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), String>;
}

pub struct SummarizationMiddleware;
impl Middleware for SummarizationMiddleware {
    fn name(&self) -> &'static str {
        "summarization"
    }
    fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), String> {
        if let Some(last) = ctx.messages.last().and_then(|v| v.as_str()) {
            let summary = last.chars().take(120).collect::<String>();
            ctx.memory_facts.push(format!("summary:{summary}"));
        }
        Ok(())
    }
}

pub struct MemoryMiddleware;
impl Middleware for MemoryMiddleware {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), String> {
        let mut facts = Vec::new();
        for msg in &ctx.messages {
            if let Some(s) = msg.as_str() {
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix("fact:") {
                        facts.push(rest.trim().to_string());
                    }
                }
            }
        }
        ctx.memory_facts.extend(facts);
        Ok(())
    }
}

pub struct TodoMiddleware;
impl Middleware for TodoMiddleware {
    fn name(&self) -> &'static str {
        "todo"
    }
    fn before_turn(&self, ctx: &mut MiddlewareContext) -> Result<(), String> {
        if ctx.configurable.is_plan_mode.unwrap_or(false) {
            ctx.todos.push("拆分子任务并跟踪状态".to_string());
            ctx.todos.push("执行并回收每个子任务结果".to_string());
        }
        let token_count = ctx.messages.iter().filter_map(|m| m.as_str()).map(str::len).sum();
        ctx.token_usage_estimate = token_count;
        ctx.loop_detected = ctx.messages.windows(2).any(|w| w.first() == w.get(1));
        Ok(())
    }
}
