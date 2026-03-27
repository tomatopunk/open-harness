use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthContext {
    pub tenant_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug)]
pub struct AuthSettings {
    pub enabled: bool,
    pub api_keys: Arc<Vec<String>>,
    pub bearer_tokens: Arc<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct AuthMiddlewareState {
    pub settings: AuthSettings,
    pub public_paths: Arc<Vec<String>>,
}

pub type SharedAuthState = Arc<RwLock<AuthMiddlewareState>>;

pub fn build_settings(
    enabled: bool,
    api_keys: Vec<String>,
    bearer_tokens: Vec<String>,
) -> AuthSettings {
    AuthSettings { enabled, api_keys: Arc::new(api_keys), bearer_tokens: Arc::new(bearer_tokens) }
}

pub fn shared_state(settings: AuthSettings, public_paths: Vec<String>) -> SharedAuthState {
    Arc::new(RwLock::new(AuthMiddlewareState { settings, public_paths: Arc::new(public_paths) }))
}

pub async fn update_settings(state: &SharedAuthState, settings: AuthSettings) {
    let mut guard = state.write().await;
    guard.settings = settings;
}

pub fn extract_auth_context(req: &Request<Body>) -> AuthContext {
    let tenant_id = req
        .headers()
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string();
    let user_id = req
        .headers()
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();
    AuthContext { tenant_id, user_id }
}

fn parse_bearer(value: &str) -> Option<&str> {
    value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))
}

fn is_authorized(req: &Request<Body>, settings: &AuthSettings) -> bool {
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        if settings.api_keys.iter().any(|k| k == v) {
            return true;
        }
    }
    if let Some(v) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = parse_bearer(v) {
            if settings.bearer_tokens.iter().any(|k| k == token) {
                return true;
            }
        }
    }
    false
}

pub async fn require_auth(
    State(state): State<SharedAuthState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let snapshot = state.read().await.clone();
    let path = req.uri().path();
    if snapshot.public_paths.iter().any(|p| p == path) {
        return next.run(req).await;
    }
    if snapshot.settings.enabled && !is_authorized(&req, &snapshot.settings) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" })))
            .into_response();
    }
    let auth_ctx = extract_auth_context(&req);
    req.extensions_mut().insert(auth_ctx);
    next.run(req).await
}
