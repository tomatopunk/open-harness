use protocol_compat::Configurable;
use runtime_kernel::{RuntimeError, RuntimeEvent, RuntimeKernel};
use serde_json::Value;

#[derive(Default)]
pub struct LanggraphAdapter {
    kernel: RuntimeKernel,
}

impl LanggraphAdapter {
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
    async fn langgraph_adapter_returns_events() {
        let adapter = LanggraphAdapter::default();
        let events = adapter
            .run(
                Configurable::default(),
                vec![serde_json::json!("fact: compatible"), serde_json::json!("search now")],
            )
            .await
            .expect("run");
        assert!(!events.is_empty());
    }
}
