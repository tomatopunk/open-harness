use protocol_compat::Configurable;
use serde_json::Value;

/// Ordered middleware hooks aligned with deer-flow lead_agent (subset).
#[derive(Debug, Clone, Default)]
pub struct MiddlewareContext {
    pub configurable: Configurable,
    pub messages: Vec<Value>,
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
    fn before_turn(&self, _ctx: &mut MiddlewareContext) -> Result<(), String> {
        Ok(())
    }
}

pub struct MemoryMiddleware;
impl Middleware for MemoryMiddleware {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn before_turn(&self, _ctx: &mut MiddlewareContext) -> Result<(), String> {
        Ok(())
    }
}

pub struct TodoMiddleware;
impl Middleware for TodoMiddleware {
    fn name(&self) -> &'static str {
        "todo"
    }
    fn before_turn(&self, _ctx: &mut MiddlewareContext) -> Result<(), String> {
        Ok(())
    }
}
