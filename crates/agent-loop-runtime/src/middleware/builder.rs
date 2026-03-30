//! Middleware Builder - helps construct middleware chains

use super::skill_injection::SkillInjectionMiddleware;
use super::tool_assembly::ToolAssemblyMiddleware;
use super::{AgentLoopMiddleware, MiddlewareChain};
use agent_ports::{SkillPort, ToolAssemblyPolicy, ToolManifest};
use std::sync::Arc;

/// 中间件链构建器
pub struct MiddlewareChainBuilder {
    middlewares: Vec<Arc<dyn AgentLoopMiddleware>>,
}

impl MiddlewareChainBuilder {
    pub fn new() -> Self {
        Self { middlewares: Vec::new() }
    }

    /// 添加工具装配中间件
    pub fn with_tool_assembly(
        mut self,
        policy: ToolAssemblyPolicy,
        all_manifests: Vec<ToolManifest>,
    ) -> Self {
        let mw = ToolAssemblyMiddleware::new(policy, all_manifests);
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// 添加 Skill 注入中间件
    pub fn with_skill_injection(mut self, skill_port: Arc<dyn SkillPort>) -> Self {
        let mw = SkillInjectionMiddleware::new(skill_port);
        self.middlewares.push(Arc::new(mw));
        self
    }

    /// 添加自定义中间件
    pub fn with_middleware(mut self, mw: Arc<dyn AgentLoopMiddleware>) -> Self {
        self.middlewares.push(mw);
        self
    }

    /// 构建中间件链
    pub fn build(self) -> MiddlewareChain {
        MiddlewareChain::new(self.middlewares)
    }
}

impl Default for MiddlewareChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}
