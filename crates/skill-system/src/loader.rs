use crate::parser::parse_skill_file;
use crate::types::{Skill, SkillError, SkillResult};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Skill 加载器
pub struct SkillLoader {
    skills_root: PathBuf,
}

impl SkillLoader {
    pub fn new(skills_root: PathBuf) -> Self {
        Self { skills_root }
    }

    /// 加载所有技能
    pub fn load_skills(&self, enabled_only: bool) -> SkillResult<Vec<Skill>> {
        let mut skills = Vec::new();

        // 扫描 public 和 custom 目录
        for category in ["public", "custom"] {
            let category_path = self.skills_root.join(category);
            if !category_path.exists() {
                debug!("Skill category directory does not exist: {}", category_path.display());
                continue;
            }

            self.walk_skills_dir(&category_path, category, &mut skills)?;
        }

        // 加载 enabled 状态
        self.load_skills_state(&mut skills)?;

        if enabled_only {
            let count = skills.iter().filter(|s| s.enabled).count();
            info!("Loaded {} enabled skills out of {} total", count, skills.len());
            skills.retain(|s| s.enabled);
        } else {
            info!("Loaded {} skills", skills.len());
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    /// 递归扫描技能目录
    fn walk_skills_dir(
        &self,
        dir: &Path,
        category: &str,
        skills: &mut Vec<Skill>,
    ) -> SkillResult<()> {
        let entries = fs::read_dir(dir).map_err(|e| {
            SkillError::ReadError(format!("Failed to read {}: {}", dir.display(), e))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() && !path.file_name().unwrap().to_str().unwrap().starts_with('.') {
                // 递归扫描子目录
                self.walk_skills_dir(&path, category, skills)?;
            } else if path.is_file() && path.file_name().unwrap() == "SKILL.md" {
                // 解析 SKILL.md
                match parse_skill_file(&path, category) {
                    Ok(skill) => {
                        debug!("Loaded skill: {} ({})", skill.name, category);
                        skills.push(skill);
                    }
                    Err(e) => {
                        warn!("Failed to parse skill at {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(())
    }

    /// 加载技能启用状态
    fn load_skills_state(&self, skills: &mut [Skill]) -> SkillResult<()> {
        // 从 governance/skills.yaml 加载 enabled 状态
        let config_path =
            self.skills_root.parent().unwrap_or(&self.skills_root).join("skills.yaml");

        if !config_path.exists() {
            debug!("Skills config not found at {}, using defaults", config_path.display());
            // 默认全部 enabled
            for skill in skills.iter_mut() {
                skill.enabled = true;
            }
            return Ok(());
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| SkillError::ReadError(format!("Failed to read config: {}", e)))?;

        let config: SkillsConfig = serde_yaml::from_str(&content)
            .map_err(|e| SkillError::ParseError(format!("Failed to parse config: {}", e)))?;

        for skill in skills.iter_mut() {
            skill.enabled = config.is_skill_enabled(&skill.name, &skill.category);
        }

        Ok(())
    }

    /// 按名称获取技能
    pub fn get_skill_by_name(&self, name: &str) -> SkillResult<Skill> {
        let skills = self.load_skills(false)?;
        skills
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| SkillError::NotFound(name.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct SkillsConfig {
    #[serde(default)]
    skills: Vec<SkillStateConfig>,
}

#[derive(Debug, Deserialize)]
struct SkillStateConfig {
    name: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

impl SkillsConfig {
    fn is_skill_enabled(&self, name: &str, _category: &str) -> bool {
        self.skills.iter().find(|s| s.name == name).map(|s| s.enabled).unwrap_or(true)
        // 默认 enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_enabled() {
        let config = SkillsConfig { skills: vec![] };
        assert!(config.is_skill_enabled("test", "public"));
    }

    #[test]
    fn test_explicit_enabled() {
        let config = SkillsConfig {
            skills: vec![SkillStateConfig { name: "test".to_string(), enabled: false }],
        };
        assert!(!config.is_skill_enabled("test", "public"));
    }
}
