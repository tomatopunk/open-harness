use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Skill 元数据（从 SKILL.md frontmatter 解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub version: String,
}

/// Skill 表示
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub skill_dir: PathBuf,
    pub skill_file: PathBuf, // SKILL.md 路径
    pub relative_path: PathBuf,
    pub category: String, // "public" 或 "custom"
    pub enabled: bool,
    pub content: String, // SKILL.md 完整内容（用于注入系统提示）
    pub manifest: SkillManifest,
}

impl Skill {
    /// 获取容器中的路径
    pub fn get_container_path(&self, container_base: &str) -> String {
        let category_base = format!("{}/{}", container_base, self.category);
        if self.relative_path.as_os_str().is_empty() {
            category_base
        } else {
            format!("{}/{}", category_base, self.relative_path.display())
        }
    }

    /// 获取技能的唯一标识
    pub fn skill_id(&self) -> String {
        format!("{}/{}", self.category, self.relative_path.display())
    }
}

/// 技能错误类型
#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("Failed to read skill file: {0}")]
    ReadError(String),

    #[error("Failed to parse skill manifest: {0}")]
    ParseError(String),

    #[error("Invalid skill format: {0}")]
    InvalidFormat(String),

    #[error("Skill not found: {0}")]
    NotFound(String),

    #[error("Failed to install skill: {0}")]
    InstallError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Security error: {0}")]
    SecurityError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type SkillResult<T> = Result<T, SkillError>;
