//! Skill Installer with security enhancements
//!
//! This installer includes security checks based on DeerFlow's implementation:
//! - Archive validation
//! - Path traversal protection
//! - Frontmatter validation
//! - Size limit enforcement

use crate::parser::parse_skill_file;
use crate::types::{Skill, SkillError, SkillResult};
use crate::validation::{
    is_unsafe_zip_member, should_ignore_archive_entry, validate_skill_archive_name,
    validate_skill_frontmatter,
};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, info, warn};
use zip::ZipArchive;

/// Maximum uncompressed size for skill archives (512 MB)
const MAX_TOTAL_SIZE: u64 = 512 * 1024 * 1024;

/// Skill installer with security hardening
pub struct SkillInstaller {
    skills_root: PathBuf,
}

impl SkillInstaller {
    pub fn new(skills_root: PathBuf) -> Self {
        Self { skills_root }
    }

    /// Install a .skill package
    pub async fn install_skill(&self, skill_package: &[u8]) -> SkillResult<Skill> {
        // Validate archive name
        validate_skill_archive_name("package.skill")?;

        // Create custom directory
        let custom_dir = self.skills_root.join("custom");
        fs::create_dir_all(&custom_dir)
            .await
            .map_err(|e| SkillError::InstallError(format!("Failed to create custom dir: {}", e)))?;

        // Open and validate archive
        let mut archive = ZipArchive::new(Cursor::new(skill_package))
            .map_err(|e| SkillError::InstallError(format!("Invalid skill package: {}", e)))?;

        // Extract to temporary directory with security checks
        let temp_dir = self.extract_archive(&mut archive, &custom_dir)?;

        // Validate the extracted skill's frontmatter
        let (is_valid, message, skill_name) = validate_skill_frontmatter(&temp_dir);
        if !is_valid {
            // Clean up on failure
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(SkillError::InstallError(format!("Invalid skill: {}", message)));
        }

        let skill_name = skill_name.ok_or_else(|| {
            SkillError::InstallError("Validation succeeded but no skill name".to_string())
        })?;

        // Validate skill name for safety
        if skill_name.contains('/') || skill_name.contains('\\') || skill_name.contains("..") {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(SkillError::InstallError(format!("Invalid skill name: {}", skill_name)));
        }

        // Move to final location
        let final_dir = custom_dir.join(&skill_name);
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).await.map_err(|e| {
                SkillError::InstallError(format!("Failed to remove existing skill: {}", e))
            })?;
        }

        fs::rename(&temp_dir, &final_dir)
            .await
            .map_err(|e| SkillError::InstallError(format!("Failed to move skill: {}", e)))?;

        info!("Successfully installed skill: {}", skill_name);

        // Parse and return the installed skill
        let skill_file = final_dir.join("SKILL.md");
        let mut installed_skill = parse_skill_file(&skill_file, "custom")?;
        installed_skill.enabled = true;

        Ok(installed_skill)
    }

    /// Extract archive with security checks
    fn extract_archive(
        &self,
        archive: &mut ZipArchive<Cursor<&[u8]>>,
        dest: &std::path::Path,
    ) -> SkillResult<PathBuf> {
        // Create temporary directory
        let temp_dir = dest.join(format!("_temp_{}", uuid::Uuid::new_v4()));

        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| SkillError::InstallError(format!("Failed to create temp dir: {}", e)))?;

        let mut total_written: u64 = 0;

        // Extract each member with security checks
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                SkillError::InstallError(format!("Failed to read archive member {}: {}", i, e))
            })?;

            let file_name = file.name().to_string();
            let is_dir = file.is_dir();

            // Skip unsafe members (path traversal, absolute paths)
            if is_unsafe_zip_member(&file_name) {
                warn!("Skipping unsafe archive member: {}", file_name);
                continue;
            }

            // Skip macOS metadata and dotfiles
            if let Some(path) = std::path::Path::new(&file_name).file_name() {
                if should_ignore_archive_entry(&std::path::PathBuf::from(path)) {
                    continue;
                }
            }

            let outpath = temp_dir.join(file.mangled_name());

            // Handle directories
            if is_dir {
                std::fs::create_dir_all(&outpath).map_err(|e| {
                    SkillError::InstallError(format!("Failed to create dir: {}", e))
                })?;
            } else {
                // Extract file content first to avoid borrow issues
                let mut content = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut content).map_err(|e| {
                    SkillError::InstallError(format!("Failed to read archive member: {}", e))
                })?;

                // Check for zip bomb
                total_written += content.len() as u64;
                if total_written > MAX_TOTAL_SIZE {
                    return Err(SkillError::InstallError(
                        "Skill archive exceeds maximum size limit (512 MB)".to_string(),
                    ));
                }

                // Create parent directories
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        SkillError::InstallError(format!("Failed to create parent: {}", e))
                    })?;
                }

                // Write the content to file
                std::fs::File::create(&outpath).and_then(|mut f| f.write_all(&content)).map_err(
                    |e| SkillError::InstallError(format!("Failed to extract file: {}", e)),
                )?;
            }

            debug!("Extracted: {} ({} bytes total)", file_name, total_written);
        }

        Ok(temp_dir)
    }

    /// Uninstall a skill
    pub async fn uninstall_skill(&self, skill_name: &str) -> SkillResult<()> {
        let skill_dir =
            self.skills_root.join("custom").join(skill_name.to_lowercase().replace(' ', "-"));

        if !skill_dir.exists() {
            return Err(SkillError::NotFound(format!("Skill {} not found", skill_name)));
        }

        std::fs::remove_dir_all(&skill_dir)
            .map_err(|e| SkillError::InstallError(format!("Failed to remove skill: {}", e)))?;

        Ok(())
    }

    /// List installed custom skills
    pub async fn list_custom_skills(&self) -> SkillResult<Vec<String>> {
        let custom_dir = self.skills_root.join("custom");

        if !custom_dir.exists() {
            return Ok(vec![]);
        }

        let mut skill_names = Vec::new();
        let mut entries = match fs::read_dir(&custom_dir).await {
            Ok(entries) => entries,
            Err(e) => {
                return Err(SkillError::ReadError(format!("Failed to read custom dir: {}", e)))
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    if let Ok(skill) = parse_skill_file(&skill_file, "custom") {
                        skill_names.push(skill.name);
                    }
                }
            }
        }

        Ok(skill_names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_list_custom_skills_empty() {
        let temp_dir = tempdir().expect("Temp dir should be created");
        let installer = SkillInstaller::new(temp_dir.path().to_path_buf());

        let skills = installer.list_custom_skills().await.expect("List skills should succeed");
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_uninstall_nonexistent_skill() {
        let temp_dir = tempdir().expect("Temp dir should be created");
        let installer = SkillInstaller::new(temp_dir.path().to_path_buf());

        let result = installer.uninstall_skill("nonexistent").await;
        assert!(result.is_err());
    }
}
