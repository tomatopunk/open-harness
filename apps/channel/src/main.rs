use axum::{routing::get, routing::post, Json, Router};
use channel_dingtalk::DingTalkDriver;
use channel_runtime::ChannelDriver;
use channel_wecom::WeComDriver;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "channel_service=info".into()),
        )
        .init();

    let ding = DingTalkDriver {
        secret: std::env::var("DINGTALK_SECRET").unwrap_or_else(|_| "dev".into()),
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/hooks/dingtalk",
            post({
                let ding = ding.clone();
                move |body: String| {
                    let ding = ding.clone();
                    async move { dingtalk_hook(ding, body).await }
                }
            }),
        )
        .route("/hooks/wecom", post(wecom_hook))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = "0.0.0.0:8082".parse()?;
    tracing::info!("open-harness-channel on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn dingtalk_hook(ding: DingTalkDriver, body: String) -> Json<serde_json::Value> {
    let headers = HashMap::new();
    let _ = ding.verify_signature(&headers, body.as_bytes()).await;
    let env = ding.parse_event(body.as_bytes()).await.unwrap();
    let cmd = ding.normalize_command(&env).await.unwrap();
    Json(json!({ "envelope": env, "command": cmd }))
}

async fn wecom_hook(body: String) -> Json<serde_json::Value> {
    let wecom = WeComDriver;
    let headers = HashMap::new();
    let _ = wecom.verify_signature(&headers, body.as_bytes()).await;
    let env = wecom.parse_event(body.as_bytes()).await.unwrap();
    let cmd = wecom.normalize_command(&env).await.unwrap();
    Json(json!({ "envelope": env, "command": cmd }))
}
