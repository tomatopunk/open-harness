use serde::{Deserialize, Serialize};

/// Runtime knobs passed through LangGraph `configurable` (aligned with deer-flow lead_agent).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Configurable {
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub thinking_enabled: Option<bool>,
    #[serde(default)]
    pub is_plan_mode: Option<bool>,
    #[serde(default)]
    pub subagent_enabled: Option<bool>,
    #[serde(default)]
    pub max_concurrent_subagents: Option<u32>,
    #[serde(default)]
    pub sandbox_enabled: Option<bool>,
    #[serde(default)]
    pub skills_enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let c = Configurable {
            thread_id: Some("t1".into()),
            model_name: Some("gpt-4".into()),
            api_key: Some("k".into()),
            thinking_enabled: Some(true),
            is_plan_mode: None,
            subagent_enabled: Some(false),
            max_concurrent_subagents: Some(4),
            sandbox_enabled: Some(true),
            skills_enabled: Some(true),
        };
        let j = serde_json::to_string(&c).unwrap();
        let back: Configurable = serde_json::from_str(&j).unwrap();
        assert_eq!(c, back);
    }
}
