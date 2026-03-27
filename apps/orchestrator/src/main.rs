use axum::{routing::get, routing::post, Json, Router};
use config_runtime::load_or_default;
use orchestrator_core::LeadPipeline;
use protocol_compat::Configurable;
use serde::Deserialize;
use serde_json::json;
use state_abstraction::LocalFsLayout;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

async fn pipeline_check() -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx = pipeline.prepare(Configurable::default()).expect("pipeline");
    Json(json!({
        "middleware": "ok",
        "configurable": ctx.configurable,
        "token_usage_estimate": ctx.token_usage_estimate
    }))
}

#[derive(Debug, Deserialize)]
struct OrchestrateRequest {
    #[serde(default)]
    configurable: Configurable,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
}

async fn run_orchestrate(Json(body): Json<OrchestrateRequest>) -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx = pipeline
        .prepare_with_input(body.configurable.clone(), body.messages.clone())
        .unwrap_or_else(|_| pipeline.prepare(body.configurable.clone()).expect("pipeline"));

    let subagent_enabled = body.configurable.subagent_enabled.unwrap_or(false);
    let max_subagents = body.configurable.max_concurrent_subagents.unwrap_or(1);
    let delegated = if subagent_enabled {
        vec![json!({
            "agent": "general",
            "status": "completed",
            "max_concurrent_subagents": max_subagents
        })]
    } else {
        Vec::new()
    };

    Json(json!({
        "ok": true,
        "loop_detected": ctx.loop_detected,
        "token_usage_estimate": ctx.token_usage_estimate,
        "todos": ctx.todos,
        "memory_facts": ctx.memory_facts,
        "delegated_subagents": delegated
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator_service=info".into()),
        )
        .init();
    let cfg = load_or_default();
    if cfg.storage.mode == "local_fs" {
        let layout = LocalFsLayout::new(&cfg.storage.local_fs_root);
        let _ = layout.ensure_base_dirs();
    }

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/internal/pipeline-check", post(pipeline_check))
        .route("/internal/orchestrate", post(run_orchestrate))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = "0.0.0.0:8083".parse()?;
    tracing::info!("open-harness-orchestrator on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
