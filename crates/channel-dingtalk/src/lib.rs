//! DingTalk webhook driver (stub: verify + parse JSON envelope).

use async_trait::async_trait;
use channel_runtime::{ChannelDriver, ChannelEnvelope, ChannelError, NormalizedCommand};
use std::collections::HashMap;

#[derive(Clone)]
pub struct DingTalkDriver {
    pub secret: String,
}

#[async_trait]
impl ChannelDriver for DingTalkDriver {
    fn platform(&self) -> &'static str {
        "dingtalk"
    }

    async fn verify_signature(
        &self,
        _headers: &HashMap<String, String>,
        _raw_body: &[u8],
    ) -> Result<(), ChannelError> {
        if self.secret.is_empty() {
            return Err(ChannelError::Unauthorized);
        }
        Ok(())
    }

    async fn parse_event(&self, raw_body: &[u8]) -> Result<ChannelEnvelope, ChannelError> {
        let v: serde_json::Value = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        Ok(ChannelEnvelope {
            platform: "dingtalk".into(),
            event_id: v.get("eventId").and_then(|x| x.as_str()).unwrap_or("unknown").to_string(),
            user_id: "user".into(),
            chat_id: "chat".into(),
            message_id: "msg".into(),
            text: v.get("text").and_then(|t| t.as_str()).map(String::from),
            attachments: vec![],
            metadata: HashMap::new(),
        })
    }

    async fn normalize_command(
        &self,
        env: &ChannelEnvelope,
    ) -> Result<NormalizedCommand, ChannelError> {
        let text = env.text.clone().unwrap_or_default();
        let cmd = if text.starts_with('/') {
            text.split_whitespace().next().unwrap_or("/help").to_string()
        } else {
            "chat".into()
        };
        Ok(NormalizedCommand { command: cmd, args: vec![], thread_hint: None })
    }

    async fn send_message(&self, _chat_id: &str, _text: &str) -> Result<(), ChannelError> {
        Ok(())
    }
}
