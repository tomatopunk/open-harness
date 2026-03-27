use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::time::timeout;
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
        let permit =
            self.permits.acquire().await.map_err(|e| RuntimeError::Subagent(e.to_string()))?;
        let fut = async move {
            let _guard = permit;
            Ok::<SubagentResult, RuntimeError>(SubagentResult {
                task_id: req.task_id,
                status: "completed".to_string(),
                output: format!("subagent:{} handled", req.agent_name),
            })
        };
        timeout(self.timeout, fut)
            .await
            .map_err(|_| RuntimeError::Subagent("subagent timeout".to_string()))?
    }
}
