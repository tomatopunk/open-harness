//! API Client - 封装 HTTP 客户端用于测试
//!
//! 提供便捷的 API 调用方法和响应断言

use anyhow::Result;
use reqwest::{Client, ClientBuilder, Response};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::debug;

/// Open Harness API 客户端
#[derive(Debug, Clone)]
pub struct OpenHarnessClient {
    client: Client,
    pub gateway_url: String,
    pub manage_url: String,
    pub channel_url: String,
}

impl Default for OpenHarnessClient {
    fn default() -> Self {
        Self::new(
            "http://127.0.0.1:8080".to_string(),
            "http://127.0.0.1:8081".to_string(),
            "http://127.0.0.1:8082".to_string(),
        )
    }
}

impl OpenHarnessClient {
    /// 创建新的 API 客户端
    pub fn new(gateway_url: String, manage_url: String, channel_url: String) -> Self {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            gateway_url,
            manage_url,
            channel_url,
        }
    }

    /// ==================== Gateway 测试 ====================
    /// Gateway 健康检查
    pub async fn gateway_health_check(&self) -> Result<Response> {
        let url = format!("{}/health", self.gateway_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 获取模型列表
    pub async fn gateway_list_models(&self) -> Result<Response> {
        let url = format!("{}/v1/models", self.gateway_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 聊天完成
    pub async fn gateway_chat_completion(
        &self,
        messages: Vec<serde_json::Value>,
    ) -> Result<Response> {
        let url = format!("{}/v1/chat/completions", self.gateway_url);
        let body = serde_json::json!({
            "model": "open-harness-default",
            "messages": messages,
            "user": "e2e-test"
        });
        debug!("POST {} {:?}", url, body);
        let resp = self.client.post(&url).json(&body).send().await?;
        Ok(resp)
    }

    /// 获取 OpenAPI spec
    pub async fn gateway_openapi(&self) -> Result<Response> {
        let url = format!("{}/openapi.json", self.gateway_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// ==================== Manage 测试 ====================
    /// Manage 健康检查
    pub async fn manage_health_check(&self) -> Result<Response> {
        let url = format!("{}/health", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 列出模型
    pub async fn manage_list_models(&self) -> Result<Response> {
        let url = format!("{}/api/models", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 列出技能
    pub async fn manage_list_skills(&self) -> Result<Response> {
        let url = format!("{}/api/skills", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 删除线程
    pub async fn manage_delete_thread(&self, thread_id: &str) -> Result<Response> {
        let url = format!("{}/api/manage/threads/{}", self.manage_url, thread_id);
        debug!("DELETE {}", url);
        let resp = self.client.delete(&url).send().await?;
        Ok(resp)
    }

    /// 获取异步操作状态
    pub async fn manage_get_thread_delete_op(&self, op_id: &str) -> Result<Response> {
        let url = format!("{}/api/manage/thread-delete-ops/{}", self.manage_url, op_id);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 获取存储状态
    pub async fn manage_storage_status(&self) -> Result<Response> {
        let url = format!("{}/api/manage/admin/storage/status", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 获取 MCP OAuth 状态
    pub async fn manage_mcp_oauth_status(&self) -> Result<Response> {
        let url = format!("{}/api/mcp/oauth/status", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 列出渠道
    pub async fn manage_list_channels(&self) -> Result<Response> {
        let url = format!("{}/api/channels", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 安装技能
    pub async fn manage_install_skill(
        &self,
        archive_name: &str,
        enabled: bool,
    ) -> Result<Response> {
        let url = format!("{}/api/skills/install", self.manage_url);
        let body = serde_json::json!({
            "archive_name": archive_name,
            "enabled": enabled
        });
        debug!("POST {} {:?}", url, body);
        let resp = self.client.post(&url).json(&body).send().await?;
        Ok(resp)
    }

    /// 创建 Agent
    pub async fn manage_create_agent(
        &self,
        name: &str,
        model: &str,
        preamble: Option<&str>,
    ) -> Result<Response> {
        let url = format!("{}/api/agents", self.manage_url);
        let body = serde_json::json!({
            "name": name,
            "model": model,
            "preamble": preamble
        });
        debug!("POST {} {:?}", url, body);
        let resp = self.client.post(&url).json(&body).send().await?;
        Ok(resp)
    }

    /// 列出 Agents
    pub async fn manage_list_agents(&self) -> Result<Response> {
        let url = format!("{}/api/agents", self.manage_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// ==================== Channels 测试 ====================
    /// Channels 健康检查
    pub async fn channels_health_check(&self) -> Result<Response> {
        let url = format!("{}/healthz", self.channel_url);
        debug!("GET {}", url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp)
    }

    /// 钉钉 Webhook
    pub async fn channels_dingtalk_hook(&self, text: &str, event_id: &str) -> Result<Response> {
        let url = format!("{}/hooks/dingtalk", self.channel_url);
        let body = serde_json::json!({
            "text": text,
            "eventId": event_id
        });
        debug!("POST {} {:?}", url, body);
        let resp = self.client.post(&url).json(&body).send().await?;
        Ok(resp)
    }

    /// 企业微信 Webhook
    pub async fn channels_wecom_hook(&self, content: &str, from_user_name: &str) -> Result<Response> {
        let url = format!("{}/hooks/wecom", self.channel_url);
        let body = serde_json::json!({
            "Content": content,
            "FromUserName": from_user_name
        });
        debug!("POST {} {:?}", url, body);
        let resp = self.client.post(&url).json(&body).send().await?;
        Ok(resp)
    }

    /// ==================== 辅助方法 ====================
    /// 解析 JSON 响应
    pub async fn parse_json<T: DeserializeOwned>(&self, resp: Response) -> Result<T> {
        let text = resp.text().await?;
        let parsed = serde_json::from_str::<T>(&text)?;
        Ok(parsed)
    }

    /// 检查响应状态码是否成功
    pub fn is_success(resp: &Response) -> bool {
        resp.status().is_success()
    }
}
