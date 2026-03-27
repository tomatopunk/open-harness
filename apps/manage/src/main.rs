use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config_runtime::load_or_default;
use metrics_exporter_prometheus::PrometheusBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

mod thread_delete;

use thread_delete::ThreadDeleteEngine;

#[derive(Clone)]
struct AppState {
    delete_engine: Arc<ThreadDeleteEngine>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "manage_service=info,tower_http=info".into()),
        )
        .init();

    let cfg = load_or_default();
    let threads_root = std::path::PathBuf::from(&cfg.manage.threads_root);
    std::fs::create_dir_all(&threads_root).ok();

    let delete_engine = ThreadDeleteEngine::new(threads_root, cfg.manage.langgraph_url.clone());

    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!("open_harness_manage_requests_total", "Manage API requests");

    let state = AppState { delete_engine };

    let app = Router::new()
        .route("/healthz", get(health))
        .route(
            "/metrics",
            get(move || {
                let p = prom.clone();
                async move { p.render() }
            }),
        )
        .route("/api/manage/threads/:thread_id", axum::routing::delete(delete_thread))
        .route("/api/manage/thread-delete-ops/:operation_id", get(get_delete_op))
        .route("/api/manage/admin/storage/switch", post(storage_switch))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.manage.bind).await?;
    tracing::info!("open-harness-manage listening on {}", cfg.manage.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn delete_thread(
    State(st): State<AppState>,
    Path(thread_id): Path<Uuid>,
) -> impl IntoResponse {
    metrics::counter!("open_harness_manage_requests_total").increment(1);
    let op_id = st.delete_engine.start_delete(thread_id);
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "operation_id": op_id,
            "thread_id": thread_id,
            "status": "accepted"
        })),
    )
}

async fn get_delete_op(
    State(st): State<AppState>,
    Path(operation_id): Path<Uuid>,
) -> impl IntoResponse {
    match st.delete_engine.get_op(operation_id) {
        Some(op) => (StatusCode::OK, Json(json!(op))).into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct StorageSwitch {
    backend: String,
}

#[derive(Debug, Serialize)]
struct StorageSwitchResponse {
    backend: String,
    applied: bool,
}

/// Phase 2: switch storage backend (stub — persists config in process only for now).
async fn storage_switch(Json(body): Json<StorageSwitch>) -> impl IntoResponse {
    (StatusCode::OK, Json(StorageSwitchResponse { backend: body.backend, applied: true }))
}
