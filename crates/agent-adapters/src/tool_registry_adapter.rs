//! Bridge `tool-runtime::ToolRegistry` to `ToolPort` with manifests.

use std::sync::Arc;
use std::time::Duration;

use agent_ports::{
    PortResult, RiskLevel, RunId, SideEffectClass, ThreadId, ToolCallSpec, ToolManifest, ToolPort,
};
use async_trait::async_trait;
use serde_json::Value;
use tool_runtime::registry::Tool;
use tool_runtime::validate_instance;
use tool_runtime::ToolRegistry;

/// Registry-backed tool port with static manifests map.
pub struct RegistryToolAdapter {
    pub registry: Arc<ToolRegistry>,
    pub manifests: Vec<ToolManifest>,
    pub default_timeout: Duration,
}

impl RegistryToolAdapter {
    #[must_use]
    pub fn new(registry: Arc<ToolRegistry>, manifests: Vec<ToolManifest>) -> Self {
        Self { registry, manifests, default_timeout: Duration::from_secs(30) }
    }
}

#[async_trait]
impl ToolPort for RegistryToolAdapter {
    fn manifests(&self) -> Vec<ToolManifest> {
        self.manifests.clone()
    }

    async fn invoke(
        &self,
        _run_id: RunId,
        _thread_id: ThreadId,
        call: &ToolCallSpec,
    ) -> PortResult<Value> {
        if let Some(m) = self.manifests.iter().find(|m| m.name == call.name) {
            if let Some(ref schema) = m.input_schema {
                validate_instance(schema, &call.args)
                    .map_err(|e| agent_ports::PortError::Tool(e.to_string()))?;
            }
        }
        self.registry
            .invoke_with_timeout(&call.name, call.args.clone(), self.default_timeout)
            .await
            .map_err(|e| agent_ports::PortError::Tool(e.to_string()))
    }
}

/// Echo tool for demos.
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn invoke(&self, args: Value) -> Result<Value, tool_runtime::ToolError> {
        Ok(args)
    }
}

/// Build default registry with echo + optional extra tools.
#[must_use]
pub fn default_echo_manifests() -> Vec<ToolManifest> {
    vec![ToolManifest {
        name: "echo".into(),
        description: Some("Echo JSON args".into()),
        input_schema: None,
        capability_tags: vec!["builtin".into()],
        risk_level: RiskLevel::Low,
        timeout_ms: 30_000,
        retry_max: 0,
        side_effect_class: SideEffectClass::None,
    }]
}
