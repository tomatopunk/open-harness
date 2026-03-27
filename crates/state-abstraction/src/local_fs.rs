use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalFsLayout {
    pub root: PathBuf,
}

impl LocalFsLayout {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    pub fn ensure_base_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.root.join("config"))?;
        std::fs::create_dir_all(self.root.join("tasks"))?;
        std::fs::create_dir_all(self.root.join("threads"))?;
        std::fs::create_dir_all(self.root.join("uploads"))?;
        std::fs::create_dir_all(self.root.join("artifacts"))?;
        std::fs::create_dir_all(self.root.join("memory"))?;
        Ok(())
    }

    pub fn thread_dir(&self, thread_id: &str) -> PathBuf {
        self.root.join("threads").join(thread_id)
    }
}
