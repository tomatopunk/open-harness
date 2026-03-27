use axum::{routing::get, routing::post, Json, Router};
use orchestrator_core::LeadPipeline;
use protocol_compat::Configurable;
use serde_json::json;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

async fn pipeline_check() -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx = pipeline.prepare(Configurable::default()).expect("pipeline");
    Json(json!({ "middleware": "ok", "configurable": ctx.configurable }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator_service=info".into()),
        )
        .init();

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/internal/pipeline-check", post(pipeline_check))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = "0.0.0.0:8083".parse()?;
    tracing::info!("open-harness-orchestrator on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
