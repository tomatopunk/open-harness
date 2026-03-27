//! WeCom (Enterprise WeChat) driver stub.

use async_trait::async_trait;
use channel_runtime::{ChannelDriver, ChannelEnvelope, ChannelError, NormalizedCommand};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct WeComDriver;

#[async_trait]
impl ChannelDriver for WeComDriver {
    fn platform(&self) -> &'static str {
        "wecom"
    }

    async fn verify_signature(
        &self,
        _headers: &HashMap<String, String>,
        _raw_body: &[u8],
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn parse_event(&self, raw_body: &[u8]) -> Result<ChannelEnvelope, ChannelError> {
        let v: serde_json::Value = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        Ok(ChannelEnvelope {
            platform: "wecom".into(),
            event_id: "evt".into(),
            user_id: v
                .get("FromUserName")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            chat_id: "chat".into(),
            message_id: "m".into(),
            text: v.get("Content").and_then(|c| c.as_str()).map(String::from),
            attachments: vec![],
            metadata: HashMap::new(),
        })
    }

    async fn normalize_command(
        &self,
        env: &ChannelEnvelope,
    ) -> Result<NormalizedCommand, ChannelError> {
        Ok(NormalizedCommand {
            command: env.text.clone().unwrap_or_else(|| "/help".into()),
            args: vec![],
            thread_hint: None,
        })
    }

    async fn send_message(&self, _chat_id: &str, _text: &str) -> Result<(), ChannelError> {
        Ok(())
    }
}
