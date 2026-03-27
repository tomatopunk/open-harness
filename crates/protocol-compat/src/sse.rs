use serde::{Deserialize, Serialize};

/// LangGraph stream modes (subset; extend as upstream evolves).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamMode {
    Values,
    Messages,
    #[serde(rename = "messages-tuple")]
    MessagesTuple,
    Updates,
    Debug,
}

/// SSE payload envelope for LangGraph-compatible streaming.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum SseEvent {
    Values(serde_json::Value),
    #[serde(rename = "messages-tuple")]
    MessagesTuple(serde_json::Value),
    End {
        run_id: String,
    },
    Error {
        message: String,
        code: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_end_json_shape() {
        let e = SseEvent::End { run_id: "run-1".into() };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["event"], "end");
        assert_eq!(v["data"]["run_id"], "run-1");
    }
}
