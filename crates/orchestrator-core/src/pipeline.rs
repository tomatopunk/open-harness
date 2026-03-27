use crate::middleware::{
    MemoryMiddleware, Middleware, MiddlewareContext, SummarizationMiddleware, TodoMiddleware,
};
use protocol_compat::Configurable;

/// Single-step pipeline applying middleware order (summarization → memory → todo).
pub struct LeadPipeline {
    chain: Vec<Box<dyn Middleware>>,
}

impl Default for LeadPipeline {
    fn default() -> Self {
        Self {
            chain: vec![
                Box::new(SummarizationMiddleware),
                Box::new(MemoryMiddleware),
                Box::new(TodoMiddleware),
            ],
        }
    }
}

impl LeadPipeline {
    pub fn prepare(&self, configurable: Configurable) -> Result<MiddlewareContext, String> {
        let mut ctx = MiddlewareContext {
            configurable,
            messages: vec![],
            memory_facts: vec![],
            todos: vec![],
            token_usage_estimate: 0,
            loop_detected: false,
        };
        self.prepare_with_messages(&mut ctx)?;
        Ok(ctx)
    }

    pub fn prepare_with_input(
        &self,
        configurable: Configurable,
        messages: Vec<serde_json::Value>,
    ) -> Result<MiddlewareContext, String> {
        let mut ctx = MiddlewareContext {
            configurable,
            messages,
            memory_facts: vec![],
            todos: vec![],
            token_usage_estimate: 0,
            loop_detected: false,
        };
        self.prepare_with_messages(&mut ctx)?;
        Ok(ctx)
    }

    fn prepare_with_messages(&self, ctx: &mut MiddlewareContext) -> Result<(), String> {
        for m in &self.chain {
            m.before_turn(ctx)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_runs() {
        let p = LeadPipeline::default();
        let c = Configurable::default();
        assert!(p.prepare(c).is_ok());
    }
}
