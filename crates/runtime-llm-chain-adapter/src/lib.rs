use protocol_compat::Configurable;
use runtime_kernel::{RuntimeError, RuntimeEvent, RuntimeKernel};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Default)]
pub struct LlmChainAdapter {
    kernel: RuntimeKernel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRunOutput {
    pub events: Vec<RuntimeEvent>,
    pub tool_calls: usize,
    pub blocked_tools: Vec<String>,
    pub loop_detected: bool,
}

impl LlmChainAdapter {
    pub async fn run(
        &self,
        configurable: Configurable,
        messages: Vec<Value>,
    ) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        Ok(self.run_with_output(configurable, messages).await?.events)
    }

    pub async fn run_with_output(
        &self,
        configurable: Configurable,
        messages: Vec<Value>,
    ) -> Result<RuntimeRunOutput, RuntimeError> {
        let ctx = self.kernel.prepare_with_input(configurable, messages).await?;
        let events = self.kernel.render_events(&ctx);
        Ok(RuntimeRunOutput {
            events,
            tool_calls: ctx.tool_calls.len(),
            blocked_tools: ctx.blocked_tools,
            loop_detected: ctx.loop_detected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn llm_chain_adapter_returns_events() {
        let adapter = LlmChainAdapter::default();
        let output = adapter
            .run_with_output(
                Configurable::default(),
                vec![serde_json::json!("fact: stable"), serde_json::json!("search something")],
            )
            .await
            .expect("run");
        assert!(!output.events.is_empty());
        assert!(!output.loop_detected);
        assert_eq!(output.blocked_tools, vec!["web_search".to_string()]);
    }

    #[tokio::test]
    async fn llm_chain_adapter_run_keeps_legacy_events_api() {
        let adapter = LlmChainAdapter::default();
        let events = adapter
            .run(
                Configurable::default(),
                vec![serde_json::json!("fact: stable"), serde_json::json!("search something")],
            )
            .await
            .expect("run");
        assert!(!events.is_empty());
    }
}
