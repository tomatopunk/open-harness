use std::time::Duration;

use protocol_compat::Configurable;
use serde_json::Value;
use uuid::Uuid;

use crate::middleware::{
    ClarificationMiddleware, MemoryMiddleware, Middleware, MiddlewareContext,
    SummarizationMiddleware, TodoMiddleware, ToolDetectMiddleware,
};
use crate::subagent::{SubagentExecutor, SubagentRequest, SubagentResult};
use crate::types::{RuntimeError, RuntimeEvent};

pub struct RuntimeKernel {
    chain: Vec<Box<dyn Middleware>>,
    subagent_executor: SubagentExecutor,
}

impl Default for RuntimeKernel {
    fn default() -> Self {
        Self {
            chain: vec![
                Box::new(SummarizationMiddleware),
                Box::new(MemoryMiddleware),
                Box::new(ToolDetectMiddleware),
                Box::new(TodoMiddleware),
                Box::new(ClarificationMiddleware),
            ],
            subagent_executor: SubagentExecutor::new(4, Duration::from_secs(30)),
        }
    }
}

impl RuntimeKernel {
    pub async fn prepare_with_input(
        &self,
        configurable: Configurable,
        messages: Vec<Value>,
    ) -> Result<MiddlewareContext, RuntimeError> {
        let mut ctx = MiddlewareContext {
            configurable,
            messages,
            memory_facts: vec![],
            todos: vec![],
            skill_hints: vec![],
            tool_calls: vec![],
            loop_detected: false,
            token_usage_estimate: 0,
        };
        for middleware in &self.chain {
            middleware.before_turn(&mut ctx).await?;
        }
        Ok(ctx)
    }

    pub async fn execute_subagent(&self, prompt: String) -> Result<SubagentResult, RuntimeError> {
        let req =
            SubagentRequest { task_id: Uuid::new_v4(), agent_name: "general".to_string(), prompt };
        self.subagent_executor.execute(req).await
    }

    pub fn render_events(&self, ctx: &MiddlewareContext) -> Vec<RuntimeEvent> {
        let mut out = Vec::new();
        for tool in &ctx.tool_calls {
            out.push(RuntimeEvent::ToolCall { invocation: tool.clone() });
        }
        out.push(RuntimeEvent::Value {
            payload: serde_json::json!({
                "loop_detected": ctx.loop_detected,
                "token_usage_estimate": ctx.token_usage_estimate,
                "todos": ctx.todos,
                "memory_facts": ctx.memory_facts
            }),
        });
        out.push(RuntimeEvent::End { reason: "completed".to_string() });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kernel_prepares_context_and_events() {
        let kernel = RuntimeKernel::default();
        let ctx = kernel
            .prepare_with_input(
                Configurable::default(),
                vec![serde_json::json!("fact: use rust"), serde_json::json!("please search docs")],
            )
            .await
            .expect("prepare");
        assert!(!ctx.memory_facts.is_empty());
        let events = kernel.render_events(&ctx);
        assert!(!events.is_empty());
    }
}
