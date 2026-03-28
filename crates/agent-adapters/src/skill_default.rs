//! Default skill injection (names only; content can be wired later).

use std::collections::HashMap;

use agent_ports::{PortResult, SkillContext, SkillInjection, SkillPort};
use async_trait::async_trait;

#[derive(Debug, Clone, Default)]
pub struct DefaultSkillAdapter {
    pub preamble: String,
    /// Extra preamble per skill name (e.g. from governance YAML).
    pub preamble_by_name: HashMap<String, String>,
}

#[async_trait]
impl SkillPort for DefaultSkillAdapter {
    async fn inject(&self, ctx: &SkillContext) -> PortResult<SkillInjection> {
        let mut text = self.preamble.clone();
        for name in &ctx.enabled_skill_names {
            if let Some(extra) = self.preamble_by_name.get(name) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(extra);
            }
        }
        Ok(SkillInjection { preamble: text, resolved_names: ctx.enabled_skill_names.clone() })
    }
}
