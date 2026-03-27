use async_trait::async_trait;
use serde_json::Value;

use crate::ModelError;

#[async_trait]
pub trait ChatModel: Send + Sync {
    async fn complete(&self, messages: Vec<Value>) -> Result<String, ModelError>;
}
