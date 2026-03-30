//! Tool Assembly Middleware - dynamically assembles available tools for each turn

use crate::budget::RunBudget;
use crate::error::AgentLoopResult;
use crate::middleware::{AgentLoopMiddleware, TurnContext};
use agent_ports::{LlmTurnOutput, ThreadId, ThreadState, ToolAssemblyPolicy, ToolManifest};
use async_trait::async_trait;
use serde_json::Value;

/// 工具装配中间件
pub struct ToolAssemblyMiddleware {
    policy: ToolAssemblyPolicy,
    all_manifests: Vec<ToolManifest>,
}

impl ToolAssemblyMiddleware {
    pub fn new(policy: ToolAssemblyPolicy, all_manifests: Vec<ToolManifest>) -> Self {
        Self { policy, all_manifests }
    }

    /// 为当前 turn 装配工具
    fn assemble_tools(&self) -> Vec<ToolManifest> {
        let resolved = self.policy.resolve(&self.all_manifests);
        resolved.into_iter().cloned().collect()
    }
}

#[async_trait]
impl AgentLoopMiddleware for ToolAssemblyMiddleware {
    async fn before_model(
        &self,
        ctx: &TurnContext,
        _state: &mut ThreadState,
        _messages_for_llm: &mut Vec<Value>,
    ) -> AgentLoopResult<()> {
        // 动态装配当前 turn 可用的工具
        let assembled_tools = self.assemble_tools();

        // 记录装配的工具数量（用于日志和调试）
        tracing::debug!("Assembled {} tools for turn", assembled_tools.len());

        Ok(())
    }
}

/// 工具装配中间件构建器
pub struct ToolAssemblyMiddlewareBuilder {
    policy: Option<ToolAssemblyPolicy>,
    all_manifests: Vec<ToolManifest>,
}

impl ToolAssemblyMiddlewareBuilder {
    pub fn new(all_manifests: Vec<ToolManifest>) -> Self {
        Self { policy: None, all_manifests }
    }

    pub fn with_policy(mut self, policy: ToolAssemblyPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn build(self) -> ToolAssemblyMiddleware {
        let policy = self.policy.unwrap_or_default();
        ToolAssemblyMiddleware::new(policy, self.all_manifests)
    }
}
