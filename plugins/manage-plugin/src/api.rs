//! Manage API endpoints

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

/// Create API router
pub fn create_router() -> Router {
    Router::new()
        .route("/api/agents", get(list_agents))
        .route("/api/agents", post(create_agent))
        .route("/api/models", get(list_models))
        .route("/api/health", get(health))
}

/// Agent response
#[derive(Debug, Serialize)]
struct Agent {
    id: String,
    name: String,
    model: String,
    created_at: u64,
}

/// List agents
async fn list_agents() -> impl IntoResponse {
    debug!("Listing agents");

    let agents = vec![Agent {
        id: "agent-1".to_string(),
        name: "Default Agent".to_string(),
        model: "gpt-4".to_string(),
        created_at: Utc::now().timestamp() as u64,
    }];

    Json(agents)
}

/// Create agent request
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CreateAgentRequest {
    name: String,
    model: String,
    preamble: Option<String>,
}

/// Create agent
async fn create_agent(Json(req): Json<CreateAgentRequest>) -> impl IntoResponse {
    debug!("Creating agent: {}", req.name);

    let agent = Agent {
        id: format!("agent-{}", Uuid::new_v4()),
        name: req.name,
        model: req.model,
        created_at: Utc::now().timestamp() as u64,
    };

    (StatusCode::CREATED, Json(agent))
}

/// Model response
#[derive(Debug, Serialize)]
struct Model {
    id: String,
    name: String,
    provider: String,
}

/// List models
async fn list_models() -> impl IntoResponse {
    debug!("Listing models");

    let models = vec![
        Model {
            id: "gpt-4".to_string(),
            name: "GPT-4".to_string(),
            provider: "openai".to_string(),
        },
        Model {
            id: "gpt-3.5-turbo".to_string(),
            name: "GPT-3.5 Turbo".to_string(),
            provider: "openai".to_string(),
        },
    ];

    Json(models)
}

/// Health check
async fn health() -> StatusCode {
    StatusCode::OK
}
