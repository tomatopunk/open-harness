use async_trait::async_trait;

use crate::SandboxError;

#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn exec(&self, command: &str) -> Result<String, SandboxError>;
}

/// Local no-op sandbox for tests.
pub struct LocalSandbox;

#[async_trait]
impl Sandbox for LocalSandbox {
    async fn exec(&self, command: &str) -> Result<String, SandboxError> {
        Ok(format!("echo {command}"))
    }
}
