use async_trait::async_trait;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::SandboxError;

#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn exec(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError>;
}

#[derive(Debug, Clone)]
pub struct SandboxRequest {
    pub command: String,
    pub timeout: Duration,
}

impl SandboxRequest {
    pub fn new(command: impl Into<String>, timeout: Duration) -> Self {
        Self { command: command.into(), timeout }
    }
}

#[derive(Debug, Clone)]
pub struct SandboxOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Local process sandbox with deterministic timeout.
pub struct LocalSandbox;

#[async_trait]
impl Sandbox for LocalSandbox {
    async fn exec(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(request.command);
        let output =
            timeout(request.timeout, cmd.output()).await.map_err(|_| SandboxError::Timeout)?;
        let output = output.map_err(|e| SandboxError::Execution(e.to_string()))?;
        Ok(SandboxOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_sandbox_execs_command() {
        let sb = LocalSandbox;
        let out = sb
            .exec(SandboxRequest::new("echo harness", Duration::from_secs(2)))
            .await
            .unwrap_or_else(|e| panic!("sandbox exec failed: {e}"));
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("harness"));
    }
}
