use crate::{AppState, AuthContext};
use axum::http::StatusCode;
use config_runtime::{resolve_env_var_ref, ModelConfig};
use protocol_compat::ThreadCreate;

pub fn model_api_key(models: &[ModelConfig], model_name: &str) -> Option<String> {
    models
        .iter()
        .find(|m| m.name == model_name)
        .and_then(|m| m.api_key.as_ref())
        .map(|v| resolve_env_var_ref(v))
        .filter(|v| !v.is_empty())
}

pub fn map_upstream_status(status: StatusCode) -> StatusCode {
    match status {
        StatusCode::UNAUTHORIZED => StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN => StatusCode::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS => StatusCode::TOO_MANY_REQUESTS,
        StatusCode::NOT_FOUND => StatusCode::BAD_GATEWAY,
        s if s.is_client_error() => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_GATEWAY,
    }
}

pub fn conversation_cache_key(auth_ctx: &AuthContext, request_user: Option<&str>) -> String {
    let user_key =
        request_user.filter(|v| !v.trim().is_empty()).unwrap_or(auth_ctx.user_id.as_str());
    format!("{}:{user_key}", auth_ctx.tenant_id)
}

pub fn extract_assistant_text(payload: &str) -> Option<String> {
    extract_assistant_texts(payload).into_iter().next()
}

pub fn extract_assistant_texts(payload: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in payload.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let data = trimmed.trim_start_matches("data:").trim();
        if data == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(content) =
                v.get("data").and_then(|d| d.get("content")).and_then(|c| c.as_str())
            {
                out.push(content.to_string());
                continue;
            }
            if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                out.push(content.to_string());
            }
        }
    }
    out
}

pub async fn ensure_thread(st: &AppState, thread_id: &str) -> Result<(), String> {
    let upstream = st.langgraph_upstream.read().await.clone();
    let url = format!("{}/threads", upstream.trim_end_matches('/'));
    let body = ThreadCreate { thread_id: Some(thread_id.to_string()), metadata: None };
    let resp = st.client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() || resp.status().as_u16() == 409 {
        Ok(())
    } else {
        Err(format!("thread create failed {}", resp.status()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_key_includes_tenant() {
        let auth = AuthContext { tenant_id: "tenant-a".into(), user_id: "u1".into() };
        let key = conversation_cache_key(&auth, Some("alice"));
        assert_eq!(key, "tenant-a:alice");
    }

    #[test]
    fn conversation_key_falls_back_to_auth_user() {
        let auth = AuthContext { tenant_id: "tenant-a".into(), user_id: "u1".into() };
        let key = conversation_cache_key(&auth, Some("   "));
        assert_eq!(key, "tenant-a:u1");
    }

    #[test]
    fn upstream_status_mapping() {
        assert_eq!(map_upstream_status(StatusCode::BAD_REQUEST), StatusCode::BAD_REQUEST);
        assert_eq!(map_upstream_status(StatusCode::UNAUTHORIZED), StatusCode::UNAUTHORIZED);
        assert_eq!(
            map_upstream_status(StatusCode::TOO_MANY_REQUESTS),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(map_upstream_status(StatusCode::NOT_FOUND), StatusCode::BAD_GATEWAY);
        assert_eq!(map_upstream_status(StatusCode::INTERNAL_SERVER_ERROR), StatusCode::BAD_GATEWAY);
    }
}
