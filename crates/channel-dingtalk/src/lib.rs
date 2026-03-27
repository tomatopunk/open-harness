//! DingTalk webhook driver (stub: verify + parse JSON envelope).

use async_trait::async_trait;
use base64::Engine;
use channel_runtime::{ChannelDriver, ChannelEnvelope, ChannelError, NormalizedCommand};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

#[derive(Clone)]
pub struct DingTalkDriver {
    pub secret: String,
    pub webhook_url: Option<String>,
    pub client: reqwest::Client,
}

#[async_trait]
impl ChannelDriver for DingTalkDriver {
    fn platform(&self) -> &'static str {
        "dingtalk"
    }

    async fn verify_signature(
        &self,
        headers: &HashMap<String, String>,
        _raw_body: &[u8],
    ) -> Result<(), ChannelError> {
        if self.secret.is_empty() {
            return Err(ChannelError::Unauthorized);
        }
        let sig =
            header_value(headers, "x-dingtalk-signature").ok_or(ChannelError::Unauthorized)?;
        let timestamp =
            header_value(headers, "x-dingtalk-timestamp").ok_or(ChannelError::Unauthorized)?;
        let payload = format!("{timestamp}\n{}", self.secret);
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes())
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        mac.update(payload.as_bytes());
        let digest = mac.finalize().into_bytes();
        let expected = base64::engine::general_purpose::STANDARD.encode(digest);
        if expected != sig {
            return Err(ChannelError::Unauthorized);
        }
        Ok(())
    }

    async fn parse_event(&self, raw_body: &[u8]) -> Result<ChannelEnvelope, ChannelError> {
        let v: serde_json::Value = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        Ok(ChannelEnvelope {
            platform: "dingtalk".into(),
            event_id: v
                .get("eventId")
                .or_else(|| v.get("event_id"))
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            user_id: v
                .pointer("/senderStaffId")
                .or_else(|| v.pointer("/senderId"))
                .and_then(|x| x.as_str())
                .unwrap_or("user")
                .to_string(),
            chat_id: v
                .pointer("/conversationId")
                .or_else(|| v.pointer("/chatId"))
                .and_then(|x| x.as_str())
                .unwrap_or("chat")
                .to_string(),
            message_id: v
                .pointer("/msgId")
                .or_else(|| v.pointer("/messageId"))
                .and_then(|x| x.as_str())
                .unwrap_or("msg")
                .to_string(),
            text: v
                .pointer("/text/content")
                .or_else(|| v.get("text"))
                .and_then(|t| t.as_str())
                .map(String::from),
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

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), ChannelError> {
        let Some(webhook_url) = &self.webhook_url else {
            return Ok(());
        };
        let body = serde_json::json!({
            "chat_id": chat_id,
            "msgtype": "text",
            "text": {"content": text}
        });
        let response = self
            .client
            .post(webhook_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::Upstream(e.to_string()))?;
        if !response.status().is_success() {
            return Err(ChannelError::Upstream(format!(
                "dingtalk send failed with status {}",
                response.status()
            )));
        }
        Ok(())
    }
}

fn header_value<'a>(headers: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    headers.get(key).or_else(|| headers.get(&key.to_ascii_lowercase())).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signature_fails_when_missing() {
        let driver = DingTalkDriver {
            secret: "s".to_string(),
            webhook_url: None,
            client: reqwest::Client::new(),
        };
        let headers = HashMap::new();
        assert!(driver.verify_signature(&headers, b"{}").await.is_err());
    }
}
