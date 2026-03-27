use serde::{Deserialize, Serialize};

use crate::Configurable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadCreate {
    pub thread_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub assistant_id: Option<String>,
    pub input: serde_json::Value,
    #[serde(default)]
    pub config: Option<RunConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunConfig {
    #[serde(default)]
    pub configurable: Option<Configurable>,
}
