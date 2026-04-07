//! Gateway API - OpenAI compatible endpoints

use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

/// 创建 API 路由
pub fn create_router() -> Router {
    Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/health", get(health))
}

/// 模型列表响应
#[derive(Debug, Serialize)]
struct ListModelsResponse {
    object: &'static str,
    data: Vec<Model>,
}

#[derive(Debug, Serialize)]
struct Model {
    id: String,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
}

/// 获取模型列表
async fn list_models() -> impl IntoResponse {
    debug!("Listing models");

    let response = ListModelsResponse {
        object: "list",
        data: vec![Model {
            id: "gpt-4".to_string(),
            object: "model",
            created: 1677610602,
            owned_by: "open-harness",
        }],
    };

    Json(response)
}

/// 聊天完成请求
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
    role: String,
    content: String,
}

/// 聊天完成响应
#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: u32,
    message: Message,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// 聊天完成
async fn chat_completions(Json(req): Json<ChatCompletionRequest>) -> impl IntoResponse {
    debug!("Chat completion request for model: {}", req.model);

    // 简单的占位响应
    let response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion",
        created: Utc::now().timestamp() as u64,
        model: req.model,
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: "Hello from Open Harness Gateway! (This is a placeholder response)"
                    .to_string(),
            },
            finish_reason: "stop",
        }],
        usage: Usage { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 },
    };

    Json(response)
}

/// 健康检查
async fn health() -> StatusCode {
    StatusCode::OK
}
