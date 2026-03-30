use agent_ports::{
    HealthStatus, PortError, PortResult, ToolCallSpec, ToolManifest, ToolProvider,
    ToolProviderType, ToolResult, ToolResultMetadata,
};
use async_trait::async_trait;
use skill_system::{Skill, SkillLoader};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

/// Skill 工具提供者
pub struct SkillToolProvider {
    skills: Vec<Skill>,
    skills_root: String,
}

impl SkillToolProvider {
    pub fn new(skills_root: String) -> PortResult<Self> {
        let loader = SkillLoader::new(std::path::PathBuf::from(&skills_root));
        let skills = loader
            .load_skills(true)
            .map_err(|e| PortError::Skill(format!("Failed to load skills: {}", e)))?;

        Ok(Self { skills, skills_root })
    }

    /// 获取启用的技能列表
    pub fn get_enabled_skills(&self) -> &[Skill] {
        &self.skills
    }

    /// 按名称获取技能
    pub fn get_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }
}

#[async_trait]
impl ToolProvider for SkillToolProvider {
    fn provider_type(&self) -> ToolProviderType {
        ToolProviderType::Skill
    }

    fn provider_name(&self) -> &str {
        "skills"
    }

    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>> {
        // Skill 本身不是工具，但可以返回元数据工具用于查询技能信息
        let manifests = vec![ToolManifest {
            name: "get_skill_content".into(),
            description: Some("Get skill content by name".into()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill to retrieve"
                    }
                },
                "required": ["skill_name"]
            })),
            capability_tags: vec!["skill".into(), "metadata".into()],
            risk_level: agent_ports::RiskLevel::Low,
            timeout_ms: 10000,
            retry_max: 0,
            side_effect_class: agent_ports::SideEffectClass::None,
            provider_type: ToolProviderType::Skill,
            provider_name: "skills".into(),
            load_path: None,
            version: None,
        }];

        Ok(manifests)
    }

    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult> {
        let start = Instant::now();

        // Skill 通过系统提示注入，但可以支持查询技能内容
        match call.name.as_str() {
            "get_skill_content" => {
                let skill_name =
                    call.args.get("skill_name").and_then(|v| v.as_str()).ok_or_else(|| {
                        PortError::Validation("Missing skill_name argument".into())
                    })?;

                let skill = self.get_skill(skill_name).ok_or_else(|| {
                    PortError::NotFound(format!("Skill {} not found", skill_name))
                })?;

                let result = serde_json::json!({
                    "name": skill.name,
                    "description": skill.description,
                    "content": skill.content,
                    "allowed_tools": skill.manifest.allowed_tools,
                    "license": skill.manifest.license,
                    "version": skill.manifest.version,
                });

                let execution_time_ms = start.elapsed().as_millis() as u64;

                Ok(ToolResult {
                    success: true,
                    data: result,
                    metadata: ToolResultMetadata {
                        tool_name: call.name.clone(),
                        provider_type: ToolProviderType::Skill,
                        provider_name: "skills".into(),
                        execution_time_ms,
                        retries: 0,
                        error_message: None,
                    },
                })
            }
            _ => Err(PortError::NotFound(format!("Unknown skill tool: {}", call.name))),
        }
    }

    async fn health_check(&self) -> PortResult<HealthStatus> {
        Ok(HealthStatus::Healthy)
    }
}
