use crate::McpOAuthConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    pub obtained_at: u64,
}

impl OAuthToken {
    /// 检查令牌是否即将过期（提前 buffer_secs 秒）
    pub fn is_expiring(&self, buffer_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time should be valid")
            .as_secs();
        now + buffer_secs >= self.obtained_at + self.expires_in
    }

    /// 获取过期时间戳
    pub fn expires_at(&self) -> u64 {
        self.obtained_at + self.expires_in
    }
}

/// OAuth 令牌管理器
pub struct OAuthTokenManager {
    tokens: RwLock<HashMap<String, OAuthToken>>,
    client: reqwest::Client,
}

impl OAuthTokenManager {
    pub fn new() -> Self {
        Self { tokens: RwLock::new(HashMap::new()), client: reqwest::Client::new() }
    }

    /// 获取访问令牌（自动刷新即将过期的令牌）
    pub async fn get_token(
        &self,
        server_name: &str,
        config: &McpOAuthConfig,
    ) -> Result<OAuthToken, crate::McpError> {
        // 检查缓存
        if let Some(token) = self.tokens.read().await.get(server_name) {
            if !token.is_expiring(60) {
                // 令牌有效，返回缓存
                return Ok(token.clone());
            }
        }

        // 需要获取新令牌
        let token = self.fetch_token(config).await?;
        self.tokens.write().await.insert(server_name.to_string(), token.clone());
        Ok(token)
    }

    /// 获取授权头
    pub async fn get_authorization_header(
        &self,
        server_name: &str,
        config: &McpOAuthConfig,
    ) -> Result<String, crate::McpError> {
        let token = self.get_token(server_name, config).await?;
        Ok(format!("{} {}", token.token_type, token.access_token))
    }

    /// 从 OAuth 服务器获取令牌
    async fn fetch_token(&self, config: &McpOAuthConfig) -> Result<OAuthToken, crate::McpError> {
        let mut req = self.client.post(&config.token_url).form(&[
            ("grant_type", &config.grant_type),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
        ]);

        if !config.scopes.is_empty() {
            req = req.form(&[("scope", &config.scopes.join(" "))]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| crate::McpError::OAuth(format!("Failed to fetch token: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(crate::McpError::OAuth(format!(
                "Token endpoint returned {}: {}",
                status, body
            )));
        }

        let mut token: OAuthToken = response.json().await.map_err(|e| {
            crate::McpError::OAuth(format!("Failed to parse token response: {}", e))
        })?;

        // 记录获取时间
        token.obtained_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time should be valid")
            .as_secs();

        Ok(token)
    }

    /// 撤销令牌
    pub async fn revoke_token(&self, server_name: &str) {
        self.tokens.write().await.remove(server_name);
    }

    /// 清除所有令牌
    pub async fn clear_all(&self) {
        self.tokens.write().await.clear();
    }
}

impl Default for OAuthTokenManager {
    fn default() -> Self {
        Self::new()
    }
}
