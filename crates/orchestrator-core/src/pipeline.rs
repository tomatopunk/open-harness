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
        let mut ctx = MiddlewareContext { configurable, messages: vec![] };
        for m in &self.chain {
            m.before_turn(&mut ctx)?;
        }
        Ok(ctx)
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
