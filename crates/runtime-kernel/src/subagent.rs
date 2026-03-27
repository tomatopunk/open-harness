use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::types::RuntimeError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRequest {
    pub task_id: Uuid,
    pub agent_name: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentResult {
    pub task_id: Uuid,
    pub status: String,
    pub output: String,
}

#[derive(Clone)]
pub struct SubagentExecutor {
    permits: Arc<Semaphore>,
    timeout: Duration,
}

impl SubagentExecutor {
    pub fn new(max_concurrent: usize, timeout: Duration) -> Self {
        let size = max_concurrent.max(1);
        Self { permits: Arc::new(Semaphore::new(size)), timeout }
    }

    pub async fn execute(&self, req: SubagentRequest) -> Result<SubagentResult, RuntimeError> {
        self.execute_with_cancel(req, CancellationToken::new()).await
    }

    pub async fn execute_with_cancel(
        &self,
        req: SubagentRequest,
        cancel: CancellationToken,
    ) -> Result<SubagentResult, RuntimeError> {
        let permit =
            self.permits.acquire().await.map_err(|e| RuntimeError::Subagent(e.to_string()))?;
        let fut = async move {
            let _guard = permit;
            if let Some(delay) = req.prompt.strip_prefix("sleep_ms:") {
                if let Ok(ms) = delay.parse::<u64>() {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
            }
            Ok::<SubagentResult, RuntimeError>(SubagentResult {
                task_id: req.task_id,
                status: "completed".to_string(),
                output: format!("subagent:{} handled", req.agent_name),
            })
        };
        let run = async {
            tokio::select! {
                _ = cancel.cancelled() => Err(RuntimeError::Subagent("subagent cancelled".to_string())),
                out = timeout(self.timeout, fut) => {
                    out.map_err(|_| RuntimeError::Subagent("subagent timeout".to_string()))?
                }
            }
        };
        run.await
    }

    pub fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_is_respected() {
        let executor = SubagentExecutor::new(1, Duration::from_secs(3));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = executor
            .execute_with_cancel(
                SubagentRequest {
                    task_id: Uuid::new_v4(),
                    agent_name: "general".to_string(),
                    prompt: "sleep_ms:100".to_string(),
                },
                cancel,
            )
            .await;
        assert!(result.is_err());
    }
}
