use axum::{
    extract::{Path, State},
    routing::get,
    routing::post,
    Json, Router,
};
use channel_bootstrap::register_builtin_channels;
use channel_runtime::ChannelRegistry;
use config_runtime::load_or_default;
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    registry: Arc<ChannelRegistry>,
    gateway_url: String,
    client: Client,
    model_name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "channel_service=info".into()),
        )
        .init();

    let mut registry = ChannelRegistry::new();
    let cfg = load_or_default();
    let registered_platforms = register_builtin_channels(&mut registry, &cfg);
    let model_name =
        cfg.models.first().map(|m| m.name.clone()).unwrap_or_else(|| "gpt-4".to_string());

    let state = AppState {
        registry: Arc::new(registry),
        gateway_url: std::env::var("OPEN_HARNESS_CHANNEL__GATEWAY_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
        client: Client::new(),
        model_name,
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/hooks/:platform", post(channel_hook))
        .layer(TraceLayer::new_for_http());
    let app = app.with_state(state);

    let addr: SocketAddr = "0.0.0.0:8082".parse()?;
    tracing::info!(?registered_platforms, "open-harness-channel on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Serialize)]
struct HookResponse {
    platform: String,
    delivered: bool,
    reply: String,
}

async fn channel_hook(
    State(st): State<AppState>,
    Path(platform): Path<String>,
    body: String,
) -> Json<serde_json::Value> {
    let Some(driver) = st.registry.get(&platform) else {
        return Json(json!({"error": "platform_not_supported"}));
    };
    let headers = HashMap::new();
    if driver.verify_signature(&headers, body.as_bytes()).await.is_err() {
        return Json(json!({"error": "invalid_signature"}));
    }
    let env = match driver.parse_event(body.as_bytes()).await {
        Ok(v) => v,
        Err(e) => return Json(json!({"error": e.to_string()})),
    };
    let cmd = match driver.normalize_command(&env).await {
        Ok(v) => v,
        Err(e) => return Json(json!({"error": e.to_string()})),
    };

    let prompt = match cmd.command.as_str() {
        "chat" => env.text.clone().unwrap_or_default(),
        _ => format!("{} {}", cmd.command, cmd.args.join(" ")),
    };
    let thread_key =
        cmd.thread_hint.clone().unwrap_or_else(|| format!("{}:{}", platform, env.chat_id));
    let gateway = if st.gateway_url.starts_with("http") {
        st.gateway_url.clone()
    } else {
        format!("http://{}", st.gateway_url)
    };
    let result = st
        .client
        .post(format!("{gateway}/v1/chat/completions"))
        .json(&json!({
            "model": st.model_name,
            "messages": [{"role":"user","content": prompt}],
            "user": thread_key,
            "stream": false
        }))
        .send()
        .await;
    let reply = match result {
        Ok(resp) => {
            let v = resp.json::<serde_json::Value>().await.unwrap_or_else(|_| json!({}));
            v.get("choices")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("ok")
                .to_string()
        }
        Err(e) => format!("gateway_error: {e}"),
    };
    let _ = driver.send_message(&env.chat_id, &reply).await;
    Json(json!(HookResponse { platform, delivered: true, reply }))
}
