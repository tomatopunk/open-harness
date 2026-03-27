use protocol_compat::Configurable;
use runtime_kernel::{RuntimeError, RuntimeEvent, RuntimeKernel};
use serde_json::Value;

#[derive(Default)]
pub struct LlmChainAdapter {
    kernel: RuntimeKernel,
}

impl LlmChainAdapter {
    pub async fn run(
        &self,
        configurable: Configurable,
        messages: Vec<Value>,
    ) -> Result<Vec<RuntimeEvent>, RuntimeError> {
        let ctx = self.kernel.prepare_with_input(configurable, messages).await?;
        Ok(self.kernel.render_events(&ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn llm_chain_adapter_returns_events() {
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
