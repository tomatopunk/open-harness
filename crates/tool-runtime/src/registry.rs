use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
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
