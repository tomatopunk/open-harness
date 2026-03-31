//! Skill validation utilities
//!
//! Pure-logic validation of SKILL.md frontmatter and skill directory structure.
//! Based on DeerFlow's validation implementation.

use crate::types::{SkillError, SkillResult};
use std::fs;
use std::path::Path;

/// Allowed properties in SKILL.md frontmatter
const ALLOWED_FRONTMATTER_PROPERTIES: &[&str] = &[
    "name",
    "description",
    "license",
    "allowed_tools",
    "metadata",
    "compatibility",
    "version",
    "author",
];

/// Validate a skill directory's SKILL.md frontmatter.
///
/// Returns:
/// Tuple of (is_valid, message, skill_name)
pub fn validate_skill_frontmatter(skill_dir: &Path) -> (bool, String, Option<String>) {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        return (false, "SKILL.md not found".to_string(), None);
    }

    let content = match fs::read_to_string(&skill_md) {
        Ok(c) => c,
        Err(e) => {
            return (false, format!("Failed to read SKILL.md: {}", e), None);
        }
    };

    if !content.starts_with("---") {
        return (false, "No YAML frontmatter found".to_string(), None);
    }

    // Extract frontmatter
    let frontmatter_text = match extract_frontmatter(&content) {
        Some(fm) => fm,
        None => {
            return (false, "Invalid frontmatter format".to_string(), None);
        }
    };

    // Parse YAML frontmatter
    let frontmatter: serde_yaml::Value = match serde_yaml::from_str(&frontmatter_text) {
        Ok(v) => v,
        Err(e) => {
            return (false, format!("Invalid YAML in frontmatter: {}", e), None);
        }
    };

    if !frontmatter.is_mapping() {
        return (false, "Frontmatter must be a YAML dictionary".to_string(), None);
    }

    let frontmatter_map = match frontmatter.as_mapping() {
        Some(map) => map,
        None => {
            return (false, "Frontmatter must be a YAML dictionary".to_string(), None);
        }
    };

    // Check for unexpected properties
    let mut unexpected_keys = Vec::new();
    for key in frontmatter_map.keys() {
        if let Some(key_str) = key.as_str() {
            if !ALLOWED_FRONTMATTER_PROPERTIES.contains(&key_str) {
                unexpected_keys.push(key_str);
            }
        }
    }

    if !unexpected_keys.is_empty() {
        return (
            false,
            format!("Unexpected key(s) in SKILL.md frontmatter: {}", unexpected_keys.join(", ")),
            None,
        );
    }

    // Check required fields
    let name_value = match frontmatter_map.get(serde_yaml::Value::String("name".to_string())) {
        Some(n) => n,
        None => {
            return (false, "Missing 'name' in frontmatter".to_string(), None);
        }
    };

    let _desc_value =
        match frontmatter_map.get(serde_yaml::Value::String("description".to_string())) {
            Some(d) => d,
            None => {
                return (false, "Missing 'description' in frontmatter".to_string(), None);
            }
        };

    // Validate name
    let name = match name_value.as_str() {
        Some(n) => n.trim(),
        None => {
            return (false, format!("Name must be a string, got {:?}", name_value), None);
        }
    };

    if name.is_empty() {
        return (false, "Name cannot be empty".to_string(), None);
    }

    // Check naming convention (hyphen-case: lowercase with hyphens)
    let name_pattern = regex::Regex::new(r"^[a-z0-9-]+$").expect("Name pattern should be valid");
    if !name_pattern.is_match(name) {
        return (
            false,
            format!(
                "Name '{}' should be hyphen-case (lowercase letters, digits, and hyphens only)",
                name
            ),
            None,
        );
    }

    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return (
            false,
            format!("Name '{}' cannot start/end with hyphen or contain consecutive hyphens", name),
            None,
        );
    }

    if name.len() > 64 {
        return (
            false,
            format!("Name is too long ({} characters). Maximum is 64 characters.", name.len()),
            None,
        );
    }

    (true, "Skill is valid!".to_string(), Some(name.to_string()))
}

/// Extract frontmatter from SKILL.md content
fn extract_frontmatter(content: &str) -> Option<String> {
    // Find first ---
    let start_idx = content.find("---")?;
    let rest = &content[start_idx + 3..];

    // Find second ---
    let end_idx = rest.find("\n---\n")?;

    Some(rest[..end_idx].to_string())
}

/// Validate skill archive name for security
pub fn validate_skill_archive_name(filename: &str) -> SkillResult<()> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(SkillError::InstallError("Invalid archive name".to_string()));
    }
    if !filename.ends_with(".skill") && !filename.ends_with(".zip") {
        return Err(SkillError::InstallError(
            "Skill archive must end with .skill or .zip".to_string(),
        ));
    }
    Ok(())
}

/// Check if a zip member path is unsafe (absolute or directory traversal)
pub fn is_unsafe_zip_member(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }

    let normalized = path.replace('\\', "/");

    // Check for absolute paths
    if normalized.starts_with('/') {
        return true;
    }

    // Check for directory traversal
    if normalized.contains("..") {
        return true;
    }

    false
}

/// Check if an archive entry should be ignored (macOS metadata, dotfiles)
pub fn should_ignore_archive_entry(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().starts_with('.') || n == "__MACOSX")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_skill_archive_name() {
        assert!(validate_skill_archive_name("test.skill").is_ok());
        assert!(validate_skill_archive_name("test.zip").is_ok());
        assert!(validate_skill_archive_name("../test.skill").is_err());
        assert!(validate_skill_archive_name("/absolute/path.skill").is_err());
        assert!(validate_skill_archive_name("test.txt").is_err());
    }

    #[test]
    fn test_is_unsafe_zip_member() {
        assert!(!is_unsafe_zip_member("normal/path.txt"));
        assert!(is_unsafe_zip_member("/absolute/path.txt"));
        assert!(is_unsafe_zip_member("../traversal.txt"));
        assert!(is_unsafe_zip_member("path/../../../etc/passwd"));
    }

    #[test]
    fn test_should_ignore_archive_entry() {
        assert!(should_ignore_archive_entry(&std::path::PathBuf::from(".DS_Store")));
        assert!(should_ignore_archive_entry(&std::path::PathBuf::from("__MACOSX")));
        assert!(!should_ignore_archive_entry(&std::path::PathBuf::from("SKILL.md")));
        assert!(!should_ignore_archive_entry(&std::path::PathBuf::from("normal/file.txt")));
    }
}
