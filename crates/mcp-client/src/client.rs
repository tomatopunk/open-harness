use crate::cache::McpToolsCache;
use crate::oauth::OAuthTokenManager;
use crate::types::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// MCP 客户端
pub struct McpClient {
    config: McpServerConfig,
    oauth_manager: Option<Arc<OAuthTokenManager>>,
    session: Arc<RwLock<Option<McpSession>>>,
}

struct McpSession {
    endpoint: String,
    headers: HashMap<String, String>,
}

impl McpClient {
    /// 创建新的 MCP 客户端
    pub async fn new(config: McpServerConfig) -> McpResult<Self> {
        let oauth_manager = if let Some(oauth) = &config.oauth {
            if oauth.enabled {
                Some(Arc::new(OAuthTokenManager::new()))
            } else {
                None
            }
        } else {
            None
        };

        let client = Self { config, oauth_manager, session: Arc::new(RwLock::new(None)) };

        Ok(client)
    }

    /// 获取服务器名称
    pub fn server_name(&self) -> &str {
        &self.config.name
    }

    /// 初始化连接
    pub async fn initialize(&self) -> McpResult<()> {
        match self.config.r#type.as_str() {
            "http" | "sse" => self.initialize_http().await,
            "stdio" => self.initialize_stdio().await,
            _ => Err(McpError::UnknownTransport(self.config.r#type.clone())),
        }
    }

    /// 初始化 HTTP/SSE 连接
    async fn initialize_http(&self) -> McpResult<()> {
        let url = self
            .config
            .url
            .as_ref()
            .ok_or_else(|| McpError::Protocol("HTTP transport requires url".to_string()))?;

        let mut headers = self.config.headers.clone();

        // 注入 OAuth 令牌
        if let Some(ref oauth_mgr) = self.oauth_manager {
            if let Some(ref oauth_config) = self.config.oauth {
                let auth_header =
                    oauth_mgr.get_authorization_header(&self.config.name, oauth_config).await?;
                headers.insert("Authorization".to_string(), auth_header);
            }
        }

        let session = McpSession { endpoint: url.clone(), headers };

        *self.session.write().await = Some(session);
        info!("Initialized HTTP MCP connection to {}", url);

        Ok(())
    }

    /// 初始化 stdio 连接（需要启动子进程）
    async fn initialize_stdio(&self) -> McpResult<()> {
        // TODO: 实现 stdio 传输，需要启动子进程并通过 stdin/stdout 通信
        // 这需要依赖 tokio::process::Command
        Err(McpError::UnknownTransport("stdio transport not yet implemented".to_string()))
    }

    /// 列出所有可用工具
    pub async fn list_tools(&self) -> McpResult<Vec<McpTool>> {
        match self.config.r#type.as_str() {
            "http" | "sse" => self.list_tools_http().await,
            "stdio" => self.list_tools_stdio().await,
            _ => Err(McpError::UnknownTransport(self.config.r#type.clone())),
        }
    }

    /// 通过 HTTP/SSE 列出工具
    async fn list_tools_http(&self) -> McpResult<Vec<McpTool>> {
        let session = self.session.read().await;
        let Some(session) = session.as_ref() else {
            return Err(McpError::Protocol("Not initialized".to_string()));
        };

        let client = reqwest::Client::new();
        let mut request = client.get(format!("{}/tools", session.endpoint));

        // 添加请求头
        for (key, value) in &session.headers {
            request = request.header(key, value);
        }

        // 如果需要，刷新 OAuth 令牌
        if let Some(ref oauth_mgr) = self.oauth_manager {
            if let Some(ref oauth_config) = self.config.oauth {
                if let Ok(auth_header) =
                    oauth_mgr.get_authorization_header(&self.config.name, oauth_config).await
                {
                    request = request.header("Authorization", auth_header);
                }
            }
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::ToolExecution(format!(
                "list_tools failed with {}: {}",
                status, body
            )));
        }

        // MCP 协议响应格式
        #[derive(Debug, serde::Deserialize)]
        struct ToolsResponse {
            tools: Vec<ToolInfo>,
        }

        #[derive(Debug, serde::Deserialize)]
        struct ToolInfo {
            name: String,
            description: Option<String>,
            #[serde(rename = "inputSchema")]
            input_schema: Value,
        }

        let tools_response: ToolsResponse = response.json().await?;

        let tools = tools_response
            .tools
            .into_iter()
            .map(|t| McpTool {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                server_name: self.config.name.clone(),
            })
            .collect();

        Ok(tools)
    }

    /// 通过 stdio 列出工具（未实现）
    async fn list_tools_stdio(&self) -> McpResult<Vec<McpTool>> {
        Err(McpError::UnknownTransport("stdio transport not yet implemented".to_string()))
    }

    /// 调用工具
    pub async fn call_tool(&self, name: &str, args: &Value) -> McpResult<Value> {
        match self.config.r#type.as_str() {
            "http" | "sse" => self.call_tool_http(name, args).await,
            "stdio" => self.call_tool_stdio(name, args).await,
            _ => Err(McpError::UnknownTransport(self.config.r#type.clone())),
        }
    }

    /// 通过 HTTP/SSE 调用工具
    async fn call_tool_http(&self, name: &str, args: &Value) -> McpResult<Value> {
        let session = self.session.read().await;
        let Some(session) = session.as_ref() else {
            return Err(McpError::Protocol("Not initialized".to_string()));
        };

        let client = reqwest::Client::new();
        let mut request = client.post(format!("{}/tools/{}/invoke", session.endpoint, name));

        // 添加请求头
        for (key, value) in &session.headers {
            request = request.header(key, value);
        }

        // 刷新 OAuth 令牌
        if let Some(ref oauth_mgr) = self.oauth_manager {
            if let Some(ref oauth_config) = self.config.oauth {
                if let Ok(auth_header) =
                    oauth_mgr.get_authorization_header(&self.config.name, oauth_config).await
                {
                    request = request.header("Authorization", auth_header);
                }
            }
        }

        request = request.json(&json!({
            "name": name,
            "arguments": args,
        }));

        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::ToolExecution(format!(
                "call_tool {} failed with {}: {}",
                name, status, body
            )));
        }

        #[derive(Debug, serde::Deserialize)]
        struct ToolCallResponse {
            content: Vec<Value>,
            #[serde(rename = "isError")]
            is_error: Option<bool>,
        }

        let result: ToolCallResponse = response.json().await?;

        if result.is_error.unwrap_or(false) {
            return Err(McpError::ToolExecution(format!("Tool {} returned error", name)));
        }

        // 合并所有 content
        let merged = if result.content.len() == 1 {
            result.content.into_iter().next().expect("Content should have one element")
        } else {
            json!(result.content)
        };

        Ok(merged)
    }

    /// 通过 stdio 调用工具（未实现）
    async fn call_tool_stdio(&self, _name: &str, _args: &Value) -> McpResult<Value> {
        Err(McpError::UnknownTransport("stdio transport not yet implemented".to_string()))
    }

    /// 健康检查
    pub async fn health_check(&self) -> HealthStatus {
        match self.list_tools().await {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => HealthStatus::Unhealthy(e.to_string()),
        }
    }
}

/// 多服务器 MCP 客户端
pub struct MultiServerMcpClient {
    clients: HashMap<String, Arc<McpClient>>,
    cache: Arc<McpToolsCache>,
}

impl MultiServerMcpClient {
    /// 创建新的多服务器客户端
    pub async fn new(
        configs: Vec<McpServerConfig>,
        config_path: Option<String>,
    ) -> McpResult<Self> {
        let mut clients = HashMap::new();

        for config in &configs {
            if !config.enabled {
                continue;
            }

            let name = config.name.clone();
            let client = McpClient::new(config.clone()).await?;

            // 初始化连接
            if let Err(e) = client.initialize().await {
                error!("Failed to initialize MCP client {}: {}", name, e);
                continue;
            }

            clients.insert(name, Arc::new(client));
        }

        info!("Initialized {} MCP clients", clients.len());

        Ok(Self { clients, cache: Arc::new(McpToolsCache::new(configs.clone(), config_path)) })
    }

    /// 获取所有工具
    pub async fn get_tools(&self) -> McpResult<Vec<McpTool>> {
        self.cache.get_tools().await
    }

    /// 调用工具
    pub async fn call_tool(&self, name: &str, args: &Value) -> McpResult<Value> {
        // 查找工具所在的服务器
        for (server_name, client) in &self.clients {
            let tools = client.list_tools().await?;
            if tools.iter().any(|t| t.name == name) {
                debug!("Calling tool {} on server {}", name, server_name);
                return client.call_tool(name, args).await;
            }
        }

        Err(McpError::ToolExecution(format!("Tool {} not found in any server", name)))
    }

    /// 获取客户端数量
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// 健康检查
    pub async fn health_check(&self) -> HealthStatus {
        if self.clients.is_empty() {
            return HealthStatus::Unhealthy("No MCP clients initialized".to_string());
        }

        // 检查所有客户端的健康状态
        let mut unhealthy = Vec::new();
        for (name, client) in &self.clients {
            if let HealthStatus::Unhealthy(e) = client.health_check().await {
                unhealthy.push(format!("{}: {}", name, e));
            }
        }

        if unhealthy.is_empty() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Degraded(unhealthy.join("; "))
        }
    }
}
