//! Structured memory payload (facts + optional user/history JSON), versioned for evolution without binding vector DBs into core.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Bump when adding/removing top-level fields in [`MemoryDocument`].
pub const MEMORY_DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// Deer-flow–style structured memory: facts list plus optional JSON blobs for user profile and history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub user: Value,
    #[serde(default)]
    pub history: Value,
}

impl Default for MemoryDocument {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_DOCUMENT_SCHEMA_VERSION,
            facts: Vec::new(),
            user: json!({}),
            history: json!({}),
        }
    }
}

impl MemoryDocument {
    /// Whether this document should be listed as “having memory” for admin APIs.
    #[must_use]
    pub fn has_any_content(&self) -> bool {
        !self.facts.is_empty()
            || !as_object_is_empty(&self.user)
            || !as_object_is_empty(&self.history)
    }
}

fn as_object_is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// Decode legacy `Vec<String>` JSON or current [`MemoryDocument`] JSON.
pub fn decode_memory_json_str(raw: &str) -> Result<MemoryDocument, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(MemoryDocument::default());
    }
    if let Ok(doc) = serde_json::from_str::<MemoryDocument>(t) {
        return Ok(doc);
    }
    let facts: Vec<String> =
        serde_json::from_str(t).map_err(|e| format!("memory json (legacy facts array): {e}"))?;
    Ok(MemoryDocument {
        schema_version: MEMORY_DOCUMENT_SCHEMA_VERSION,
        facts,
        user: json!({}),
        history: json!({}),
    })
}

/// Decode from a JSON value (Postgres JSON column).
pub fn decode_memory_json_value(v: &Value) -> Result<MemoryDocument, String> {
    if v.is_null() {
        return Ok(MemoryDocument::default());
    }
    decode_memory_json_str(&serde_json::to_string(v).map_err(|e| e.to_string())?)
}
