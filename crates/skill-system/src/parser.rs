use crate::types::{Skill, SkillError, SkillManifest, SkillResult};
use std::fs;
use std::path::PathBuf;

/// 解析 SKILL.md 文件
pub fn parse_skill_file(path: &std::path::Path, category: &str) -> SkillResult<Skill> {
    let content = fs::read_to_string(path)
        .map_err(|e| SkillError::ReadError(format!("Failed to read {}: {}", path.display(), e)))?;

    // 解析 YAML frontmatter
    let (frontmatter, _content_body) =
        extract_frontmatter(&content).map_err(SkillError::InvalidFormat)?;

    let manifest: SkillManifest = serde_yaml::from_str(frontmatter)
        .map_err(|e| SkillError::ParseError(format!("Failed to parse YAML: {}", e)))?;

    let skill_dir = path
        .parent()
        .ok_or_else(|| SkillError::InvalidFormat("SKILL.md has no parent directory".to_string()))?
        .to_path_buf();

    let skills_root = get_skills_root_path();
    let relative_path = skill_dir
        .strip_prefix(&skills_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| PathBuf::new());

    Ok(Skill {
        name: manifest.name.clone(),
        description: manifest.description.clone(),
        license: manifest.license.clone(),
        skill_dir,
        skill_file: path.to_path_buf(),
        relative_path,
        category: category.to_string(),
        enabled: false, // 由配置决定
        content,
        manifest,
    })
}

/// 提取 YAML frontmatter
fn extract_frontmatter(content: &str) -> Result<(&str, &str), String> {
    if !content.starts_with("---") {
        return Err("SKILL.md must start with ---".to_string());
    }

    let rest = &content[4..];
    let end_idx =
        rest.find("\n---\n").ok_or_else(|| "SKILL.md frontmatter not closed".to_string())?;

    let frontmatter = &rest[..end_idx];
    let body = &rest[end_idx + 5..];

    Ok((frontmatter, body))
}

/// 获取 skills 根目录路径
fn get_skills_root_path() -> PathBuf {
    // 从环境变量或默认路径获取
    std::env::var("HARNESS_SKILLS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("governance/skills"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter_valid() {
        let content = r#"---
name: Test Skill
description: A test skill
license: MIT
---

# Skill Content
This is the content.
"#;

        let (frontmatter, body) = extract_frontmatter(content).expect("Should extract frontmatter");
        assert_eq!(frontmatter.trim(), "name: Test Skill\ndescription: A test skill\nlicense: MIT");
        assert_eq!(body.trim(), "# Skill Content\nThis is the content.");
    }

    #[test]
    fn test_extract_frontmatter_missing_end() {
        let content = r#"---
name: Test
"#;
        assert!(extract_frontmatter(content).is_err());
    }

    #[test]
    fn test_extract_frontmatter_no_start() {
        let content = r#"name: Test
---
content
"#;
        assert!(extract_frontmatter(content).is_err());
    }
}
