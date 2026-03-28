use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio::time::timeout;

use crate::ToolError;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    async fn invoke(&self, args: Value) -> Result<Value, ToolError>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Every registered tool name must appear in `manifest_tool_names`, and every manifest name must be registered.
    pub fn ensure_consistent_with_manifest_list(
        &self,
        manifest_tool_names: &[String],
    ) -> Result<(), ToolError> {
        let set: HashSet<&str> = manifest_tool_names.iter().map(String::as_str).collect();
        for reg_name in self.tools.keys() {
            if !set.contains(reg_name.as_str()) {
                return Err(ToolError::Execution(format!(
                    "registry has tool {reg_name} not listed in governance tool manifests"
                )));
            }
        }
        for m in manifest_tool_names {
            if !self.tools.contains_key(m) {
                return Err(ToolError::Execution(format!(
                    "tools.yaml lists tool {m} but it is not registered in ToolRegistry"
                )));
            }
        }
        Ok(())
    }

    pub async fn invoke_with_timeout(
        &self,
        name: &str,
        args: Value,
        dur: Duration,
    ) -> Result<Value, ToolError> {
        let Some(t) = self.tools.get(name) else {
            return Err(ToolError::Execution(format!("unknown tool {name}")));
        };
        timeout(dur, t.invoke(args)).await.map_err(|_| ToolError::Timeout)?
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
