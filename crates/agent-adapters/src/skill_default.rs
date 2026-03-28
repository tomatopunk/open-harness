//! Default skill injection (names only; content can be wired later).

use agent_ports::{PortResult, SkillContext, SkillInjection, SkillPort};
use async_trait::async_trait;

#[derive(Debug, Default, Clone)]
pub struct DefaultSkillAdapter {
    pub preamble: String,
}

#[async_trait]
impl SkillPort for DefaultSkillAdapter {
    async fn inject(&self, ctx: &SkillContext) -> PortResult<SkillInjection> {
        Ok(SkillInjection {
            preamble: self.preamble.clone(),
            resolved_names: ctx.enabled_skill_names.clone(),
        })
    }
}
