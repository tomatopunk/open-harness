//! Skill Injection Middleware - injects skill preambles into system prompt

use crate::error::AgentLoopResult;
use crate::middleware::{AgentLoopMiddleware, TurnContext};
use agent_ports::{SkillContext, SkillPort, ThreadState};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Skill 注入中间件
pub struct SkillInjectionMiddleware {
    skill_port: Arc<dyn SkillPort>,
}

impl SkillInjectionMiddleware {
    pub fn new(skill_port: Arc<dyn SkillPort>) -> Self {
        Self { skill_port }
    }
}

#[async_trait]
impl AgentLoopMiddleware for SkillInjectionMiddleware {
    async fn before_model(
        &self,
        ctx: &TurnContext,
        _state: &mut ThreadState,
        messages_for_llm: &mut Vec<Value>,
    ) -> AgentLoopResult<()> {
        // 准备 Skill 上下文
        let skill_ctx = SkillContext {
            thread_id: ctx.thread_id,
            enabled_skill_names: ctx.run_cfg.enabled_skill_names.clone(),
        };

        // 获取 Skill 注入内容
        let injection = self.skill_port.inject(&skill_ctx).await?;

        // 注入到系统提示
        if let Some(ref mut system_prompt) = messages_for_llm.first_mut() {
            if let Some(system_text) = system_prompt.get_mut("content") {
                if let Some(text) = system_text.as_str() {
                    let new_content = format!("{}\n\n{}", text, injection.preamble);
                    *system_text = Value::String(new_content);
                }
            }
        }

        tracing::debug!("Injected skill preamble ({} skills)", injection.resolved_names.len());

        Ok(())
    }
}

/// Skill 注入中间件构建器
pub struct SkillInjectionMiddlewareBuilder {
    skill_port: Option<Arc<dyn SkillPort>>,
}

impl SkillInjectionMiddlewareBuilder {
    pub fn new() -> Self {
        Self { skill_port: None }
    }

    pub fn with_skill_port(mut self, skill_port: Arc<dyn SkillPort>) -> Self {
        self.skill_port = Some(skill_port);
        self
    }

    pub fn build(self) -> Option<SkillInjectionMiddleware> {
        self.skill_port.map(SkillInjectionMiddleware::new)
    }
}

impl Default for SkillInjectionMiddlewareBuilder {
    fn default() -> Self {
        Self::new()
    }
}
