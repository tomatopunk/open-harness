use protocol_compat::Configurable;
use runtime_kernel::{RuntimeError, RuntimeEvent, RuntimeKernel};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Frozen JSON schema version for [`RuntimeRunOutput`] (bump on breaking field changes).
pub const RUNTIME_OUTPUT_SCHEMA_VERSION: u32 = 1;

#[derive(Default)]
pub struct LlmChainAdapter {
    kernel: RuntimeKernel,
}

/// Deterministic metadata emitted alongside events for orchestrator and metrics (plan H2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRunMetadata {
    pub schema_version: u32,
    pub engine: String,
    pub loop_detected: bool,
    pub token_usage_estimate: usize,
    pub tool_calls: usize,
    pub blocked_tools: Vec<String>,
    pub warnings: Vec<String>,
    pub todos_count: usize,
    pub memory_facts_count: usize,
}

impl Default for RuntimeRunMetadata {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_OUTPUT_SCHEMA_VERSION,
            engine: "llm-chain".to_string(),
            loop_detected: false,
            token_usage_estimate: 0usize,
            tool_calls: 0,
            blocked_tools: Vec::new(),
            warnings: Vec::new(),
            todos_count: 0,
            memory_facts_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRunOutput {
    pub metadata: RuntimeRunMetadata,
    pub events: Vec<RuntimeEvent>,
    /// Legacy flat fields (kept for backward compatibility with earlier consumers).
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
        let metadata = RuntimeRunMetadata {
            schema_version: RUNTIME_OUTPUT_SCHEMA_VERSION,
            engine: "llm-chain".to_string(),
            loop_detected: ctx.loop_detected,
            token_usage_estimate: ctx.token_usage_estimate,
            tool_calls: ctx.tool_calls.len(),
            blocked_tools: ctx.blocked_tools.clone(),
            warnings: ctx.warnings.clone(),
            todos_count: ctx.todos.len(),
            memory_facts_count: ctx.memory_facts.len(),
        };
        Ok(RuntimeRunOutput {
            metadata,
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
        assert_eq!(output.metadata.schema_version, RUNTIME_OUTPUT_SCHEMA_VERSION);
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
