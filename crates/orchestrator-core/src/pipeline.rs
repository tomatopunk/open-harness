use crate::middleware::MiddlewareContext;
use protocol_compat::Configurable;
use runtime_kernel::{RuntimeError, RuntimeKernel};

#[derive(Default)]
pub struct LeadPipeline {
    kernel: RuntimeKernel,
}

impl LeadPipeline {
    pub async fn prepare(
        &self,
        configurable: Configurable,
    ) -> Result<MiddlewareContext, RuntimeError> {
        self.kernel.prepare_with_input(configurable, vec![]).await
    }

    pub async fn prepare_with_input(
        &self,
        configurable: Configurable,
        messages: Vec<serde_json::Value>,
    ) -> Result<MiddlewareContext, RuntimeError> {
        self.kernel.prepare_with_input(configurable, messages).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pipeline_runs() {
        let p = LeadPipeline::default();
        let c = Configurable::default();
        assert!(p.prepare_with_input(c, vec![serde_json::json!("hello")]).await.is_ok());
    }
}
