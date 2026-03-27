//! WeCom (Enterprise WeChat) driver stub.

use async_trait::async_trait;
use base64::Engine;
use channel_runtime::{ChannelDriver, ChannelEnvelope, ChannelError, NormalizedCommand};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;

#[derive(Clone)]
pub struct WeComDriver {
    pub secret: Option<String>,
    pub webhook_url: Option<String>,
    pub client: reqwest::Client,
}

#[async_trait]
impl ChannelDriver for WeComDriver {
    fn platform(&self) -> &'static str {
        "wecom"
    }

    async fn verify_signature(
        &self,
        headers: &HashMap<String, String>,
        raw_body: &[u8],
    ) -> Result<(), ChannelError> {
        let Some(secret) = &self.secret else {
            return Ok(());
        };
        let signature =
            header_value(headers, "x-wecom-signature").ok_or(ChannelError::Unauthorized)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        mac.update(raw_body);
        let expected =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        if expected != signature {
            return Err(ChannelError::Unauthorized);
        }
        Ok(())
    }

    async fn parse_event(&self, raw_body: &[u8]) -> Result<ChannelEnvelope, ChannelError> {
        let v: serde_json::Value = serde_json::from_slice(raw_body)
            .map_err(|e| ChannelError::InvalidPayload(e.to_string()))?;
        Ok(ChannelEnvelope {
            platform: "wecom".into(),
            event_id: v
                .get("MsgId")
                .or_else(|| v.get("EventKey"))
                .and_then(|x| x.as_str())
                .unwrap_or("evt")
                .to_string(),
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

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), ChannelError> {
        let Some(webhook_url) = &self.webhook_url else {
            return Ok(());
        };
        let body = serde_json::json!({
            "chatid": chat_id,
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
                "wecom send failed with status {}",
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
    async fn accepts_when_secret_disabled() {
        let driver =
            WeComDriver { secret: None, webhook_url: None, client: reqwest::Client::new() };
        let headers = HashMap::new();
        assert!(driver.verify_signature(&headers, b"{}").await.is_ok());
    }
}
