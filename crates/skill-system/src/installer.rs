use crate::parser::parse_skill_file;
use crate::types::{Skill, SkillError, SkillResult};
use std::io::{self, Cursor};
use std::path::PathBuf;
use tokio::fs;
use zip::ZipArchive;

/// Skill 安装器
pub struct SkillInstaller {
    skills_root: PathBuf,
}

impl SkillInstaller {
    pub fn new(skills_root: PathBuf) -> Self {
        Self { skills_root }
    }

    /// 安装 .skill 包
    pub async fn install_skill(&self, skill_package: &[u8]) -> SkillResult<Skill> {
        // 验证包名（这里简化处理，实际应该从包内读取）
        Self::validate_skill_archive_name("package.skill")?;

        // 解压到 custom 目录
        let custom_dir = self.skills_root.join("custom");
        fs::create_dir_all(&custom_dir)
            .await
            .map_err(|e| SkillError::InstallError(format!("Failed to create custom dir: {}", e)))?;

        // 解压 zip 包
        let archive = ZipArchive::new(Cursor::new(skill_package))
            .map_err(|e| SkillError::InstallError(format!("Invalid skill package: {}", e)))?;

        let temp_dir = self.extract_archive(archive, &custom_dir)?;

        // 验证解压后的技能
        let skill = self.validate_installed_skill(&temp_dir)?;

        // 移动到最终位置
        let final_dir = custom_dir.join(&skill.name.to_lowercase().replace(' ', "-"));
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).await.map_err(|e| {
                SkillError::InstallError(format!("Failed to remove existing skill: {}", e))
            })?;
        }

        fs::rename(&temp_dir, &final_dir)
            .await
            .map_err(|e| SkillError::InstallError(format!("Failed to move skill: {}", e)))?;

        // 重新加载技能
        let skill_file = final_dir.join("SKILL.md");
        let mut installed_skill = parse_skill_file(&skill_file, "custom")?;
        installed_skill.enabled = true; // 新安装的技能默认启用

        Ok(installed_skill)
    }

    /// 验证包名
    fn validate_skill_archive_name(filename: &str) -> SkillResult<()> {
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

    /// 解压到临时目录
    fn extract_archive(
        &self,
        mut archive: ZipArchive<Cursor<&[u8]>>,
        dest: &std::path::Path,
    ) -> SkillResult<PathBuf> {
        // 创建临时目录
        let temp_dir = dest.join(format!("_temp_{}", uuid::Uuid::new_v4()));

        // 使用阻塞 IO 因为这在 zip 迭代器内部
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| SkillError::InstallError(format!("Failed to create temp dir: {}", e)))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let outpath = temp_dir.join(file.mangled_name());

            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath).map_err(|e| {
                    SkillError::InstallError(format!("Failed to create dir: {}", e))
                })?;
            } else {
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        SkillError::InstallError(format!("Failed to create parent: {}", e))
                    })?;
                }

                let mut outfile = std::fs::File::create(&outpath).map_err(|e| {
                    SkillError::InstallError(format!("Failed to create file: {}", e))
                })?;

                io::copy(&mut file, &mut outfile).map_err(|e| {
                    SkillError::InstallError(format!("Failed to extract file: {}", e))
                })?;
            }
        }

        Ok(temp_dir)
    }

    /// 验证安装的 skill
    fn validate_installed_skill(&self, dir: &std::path::Path) -> SkillResult<Skill> {
        let skill_file = dir.join("SKILL.md");

        if !skill_file.exists() {
            return Err(SkillError::InstallError("SKILL.md not found in package".to_string()));
        }

        parse_skill_file(&skill_file, "custom")
    }

    /// 卸载技能
    pub async fn uninstall_skill(&self, skill_name: &str) -> SkillResult<()> {
        let skill_dir =
            self.skills_root.join("custom").join(skill_name.to_lowercase().replace(' ', "-"));

        if !skill_dir.exists() {
            return Err(SkillError::NotFound(format!("Skill {} not found", skill_name)));
        }

        // 使用阻塞 IO
        std::fs::remove_dir_all(&skill_dir)
            .map_err(|e| SkillError::InstallError(format!("Failed to remove skill: {}", e)))?;

        Ok(())
    }

    /// 列出已安装的自定义技能
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
