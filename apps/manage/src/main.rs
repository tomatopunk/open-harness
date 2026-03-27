use app_auth::{
    build_settings, require_auth, shared_state, update_settings, AuthContext, AuthSettings,
    SharedAuthState,
};
use axum::{
    body::Body,
    extract::{Extension, Multipart, Path, Query, State},
    http::{header, StatusCode},
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use channel_bootstrap::configured_channels;
use config_runtime::{load_cached_or_default, reload_cached};
use hmac::{Hmac, Mac};
use metrics_exporter_prometheus::PrometheusBuilder;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use state_abstraction::{
    LocalFsStateStore, MemoryStore, SkillRecord, SkillStore, StorageBackendKind,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

mod tasks;
mod thread_delete;

use tasks::{bump_task, is_terminal, prune_tasks, MAX_STREAM_CHUNKS};
use thread_delete::ThreadDeleteEngine;

#[derive(Clone)]
struct AppState {
    delete_engine: Arc<ThreadDeleteEngine>,
    threads_root: PathBuf,
    local_fs_root: PathBuf,
    store: Arc<RwLock<ManageStore>>,
    storage: Arc<LocalFsStateStore>,
    tasks: Arc<dashmap::DashMap<String, TaskRecord>>,
    task_capacity: usize,
    langgraph_url: Arc<RwLock<String>>,
    http_client: reqwest::Client,
    webhook_secret: Option<String>,
    auth_state: SharedAuthState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRecord {
    task_id: String,
    thread_id: String,
    status: TaskStatus,
    created_at: i64,
    updated_at: i64,
    version: u64,
    output_chunks: Vec<String>,
    error: Option<String>,
    callback_url: Option<String>,
    stream: bool,
    client_task_id: Option<String>,
    tenant_id: String,
    user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskDispatchRequest {
    input: serde_json::Value,
    #[serde(default)]
    configurable: Option<serde_json::Value>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    callback_url: Option<String>,
    #[serde(default)]
    client_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelInfo {
    name: String,
    model: String,
    display_name: String,
    description: String,
    supports_thinking: bool,
    supports_reasoning_effort: bool,
}

#[derive(Debug, Clone, Default)]
struct ManageStore {
    mcp_servers: serde_json::Value,
    agents: HashMap<String, serde_json::Value>,
    channels: HashMap<String, String>,
    models: Vec<ModelInfo>,
    storage_mode: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "manage_service=info,tower_http=info".into()),
        )
        .init();

    let cfg = load_cached_or_default();
    let use_local_fs = cfg.storage.mode == "local_fs";
    let local_fs_root = if use_local_fs {
        PathBuf::from(&cfg.storage.local_fs_root)
    } else {
        PathBuf::from(&cfg.manage.threads_root)
    };
    let threads_root = local_fs_root.join("threads");
    std::fs::create_dir_all(&threads_root).ok();
    if use_local_fs {
        std::fs::create_dir_all(local_fs_root.join("config")).ok();
        std::fs::create_dir_all(local_fs_root.join("tasks")).ok();
        std::fs::create_dir_all(local_fs_root.join("uploads")).ok();
        std::fs::create_dir_all(local_fs_root.join("artifacts")).ok();
        std::fs::create_dir_all(local_fs_root.join("memory")).ok();
    }

    let delete_engine =
        ThreadDeleteEngine::new(threads_root.clone(), cfg.manage.langgraph_url.clone());

    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!("open_harness_manage_requests_total", "Manage API requests");
    metrics::describe_counter!("open_harness_manage_task_created_total", "Manage task created");
    metrics::describe_counter!("open_harness_manage_task_completed_total", "Manage task completed");
    metrics::describe_counter!("open_harness_manage_task_failed_total", "Manage task failed");
    metrics::describe_counter!(
        "open_harness_manage_webhook_success_total",
        "Manage webhook success"
    );
    metrics::describe_counter!(
        "open_harness_manage_webhook_failure_total",
        "Manage webhook failure"
    );

    let channels =
        configured_channels(&cfg).into_iter().map(|name| (name, "running".to_string())).collect();
    let storage = Arc::new(LocalFsStateStore::new(local_fs_root.clone()));
    let auth_settings = auth_settings_from_config(
        cfg.manage.auth.enabled,
        cfg.manage.auth.api_keys.clone(),
        cfg.manage.auth.bearer_tokens.clone(),
    );
    let auth_state =
        shared_state(auth_settings, vec!["/healthz".to_string(), "/metrics".to_string()]);

    let state = AppState {
        delete_engine,
        threads_root: threads_root.clone(),
        local_fs_root: local_fs_root.clone(),
        storage: storage.clone(),
        tasks: Arc::new(dashmap::DashMap::new()),
        task_capacity: 1000,
        langgraph_url: Arc::new(RwLock::new(cfg.manage.langgraph_url.clone())),
        http_client: reqwest::Client::new(),
        webhook_secret: cfg.manage.webhook_secret.clone(),
        auth_state: auth_state.clone(),
        store: Arc::new(RwLock::new(ManageStore {
            mcp_servers: json!({}),
            agents: HashMap::new(),
            channels,
            models: cfg
                .models
                .iter()
                .map(|m| ModelInfo {
                    name: m.name.clone(),
                    model: m.model.clone(),
                    display_name: m.display_name.clone(),
                    description: format!("provider: {}", m.use_provider),
                    supports_thinking: false,
                    supports_reasoning_effort: false,
                })
                .collect(),
            storage_mode: cfg.storage.mode.clone(),
        })),
    };
    bootstrap_storage(&state).await;

    let app = Router::new()
        .route("/healthz", get(health))
        .route(
            "/metrics",
            get(move || {
                let p = prom.clone();
                async move { p.render() }
            }),
        )
        .route("/api/models", get(list_models))
        .route("/api/models/:model_name", get(get_model))
        .route("/api/mcp/config", get(get_mcp_config).put(put_mcp_config))
        .route("/api/memory", get(get_memory))
        .route("/api/memory/reload", post(reload_memory))
        .route("/api/memory/config", get(get_memory_config))
        .route("/api/memory/status", get(get_memory_status))
        .route("/api/skills", get(list_skills))
        .route("/api/skills/:skill_name", get(get_skill).put(update_skill))
        .route("/api/threads/:thread_id", axum::routing::delete(delete_thread_plain))
        .route("/api/threads/:thread_id/uploads", post(upload_thread_files))
        .route("/api/threads/:thread_id/uploads/list", get(list_thread_uploads))
        .route(
            "/api/threads/:thread_id/uploads/:filename",
            axum::routing::delete(delete_thread_upload),
        )
        .route("/api/threads/:thread_id/artifacts/*path", get(get_thread_artifact))
        .route("/api/threads/:thread_id/suggestions", post(post_suggestions))
        .route("/api/agents", get(list_agents).post(create_agent))
        .route("/api/agents/check", get(check_agent_name))
        .route("/api/agents/:name", get(get_agent).put(update_agent).delete(delete_agent))
        .route("/api/user-profile", get(get_user_profile).put(put_user_profile))
        .route("/api/channels/", get(get_channels))
        .route("/api/channels/:name/restart", post(restart_channel))
        .route("/api/manage/threads/:thread_id", axum::routing::delete(delete_thread))
        .route("/api/manage/thread-delete-ops/:operation_id", get(get_delete_op))
        .route("/api/manage/threads/:thread_id/tasks", post(dispatch_task))
        .route("/api/manage/tasks/:task_id", get(get_task))
        .route("/api/manage/tasks/:task_id/stream", get(stream_task))
        .route("/api/manage/admin/storage/switch", post(storage_switch))
        .route("/api/manage/admin/config/reload", post(reload_config))
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_auth))
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

async fn delete_thread_plain(
    State(st): State<AppState>,
    Path(thread_id): Path<Uuid>,
) -> impl IntoResponse {
    delete_thread(State(st), Path(thread_id)).await
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

fn auth_settings_from_config(
    enabled: bool,
    api_keys: Vec<String>,
    bearer_tokens: Vec<String>,
) -> AuthSettings {
    build_settings(enabled, api_keys, bearer_tokens)
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn sign_payload(secret: &str, body: &str, timestamp: i64, nonce: &str) -> Option<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    let payload = format!("{timestamp}.{nonce}.{body}");
    mac.update(payload.as_bytes());
    Some(BASE64_STANDARD.encode(mac.finalize().into_bytes()))
}

async fn post_webhook(st: &AppState, task: &TaskRecord) {
    let Some(callback_url) = task.callback_url.clone() else {
        return;
    };
    if Url::parse(&callback_url).is_err() {
        tracing::warn!(task_id = %task.task_id, "invalid callback_url");
        return;
    }
    let payload = json!({
        "task_id": task.task_id,
        "thread_id": task.thread_id,
        "status": task.status,
        "output_chunks": task.output_chunks,
        "error": task.error,
        "tenant_id": task.tenant_id,
        "user_id": task.user_id,
        "updated_at": task.updated_at
    });
    let body = payload.to_string();
    let timestamp = now_ts();
    let nonce = Uuid::new_v4().to_string();
    let mut rb = st.http_client.post(callback_url).header("content-type", "application/json");
    if let Some(secret) = st.webhook_secret.as_ref() {
        if let Some(signature) = sign_payload(secret, &body, timestamp, &nonce) {
            rb = rb
                .header("x-open-harness-signature", signature)
                .header("x-open-harness-timestamp", timestamp.to_string())
                .header("x-open-harness-nonce", nonce);
        }
    }
    match rb.body(body).send().await {
        Ok(resp) if resp.status().is_success() => {
            metrics::counter!("open_harness_manage_webhook_success_total").increment(1);
        }
        Ok(resp) => {
            metrics::counter!("open_harness_manage_webhook_failure_total").increment(1);
            tracing::warn!(task_id = %task.task_id, status = %resp.status(), "webhook call failed");
        }
        Err(err) => {
            metrics::counter!("open_harness_manage_webhook_failure_total").increment(1);
            tracing::warn!(task_id = %task.task_id, error = %err, "webhook call error");
        }
    }
}

async fn dispatch_task(
    State(st): State<AppState>,
    Extension(auth_ctx): Extension<AuthContext>,
    Path(thread_id): Path<String>,
    Json(body): Json<TaskDispatchRequest>,
) -> impl IntoResponse {
    metrics::counter!("open_harness_manage_requests_total").increment(1);
    metrics::counter!("open_harness_manage_task_created_total").increment(1);
    let task_id = body.client_task_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = now_ts();
    let task = TaskRecord {
        task_id: task_id.clone(),
        thread_id: thread_id.clone(),
        status: TaskStatus::Queued,
        created_at: now,
        updated_at: now,
        version: 1,
        output_chunks: Vec::new(),
        error: None,
        callback_url: body.callback_url.clone(),
        stream: body.stream,
        client_task_id: body.client_task_id.clone(),
        tenant_id: auth_ctx.tenant_id.clone(),
        user_id: auth_ctx.user_id.clone(),
    };
    st.tasks.insert(task_id.clone(), task.clone());
    prune_tasks(&st.tasks, st.task_capacity);
    let st_clone = st.clone();
    let spawned_task_id = task_id.clone();
    tokio::spawn(async move {
        run_task_worker(st_clone, spawned_task_id, thread_id, body).await;
    });
    (StatusCode::ACCEPTED, Json(json!({"task_id": task_id, "status": "queued"})))
}

async fn get_task(State(st): State<AppState>, Path(task_id): Path<String>) -> impl IntoResponse {
    if let Some(task) = st.tasks.get(&task_id) {
        return (StatusCode::OK, Json(json!(task.clone()))).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({"error":"task_not_found"}))).into_response()
}

async fn stream_task(State(st): State<AppState>, Path(task_id): Path<String>) -> impl IntoResponse {
    let body_stream = futures::stream::unfold(
        (st, task_id, 0_u64, false),
        |(st, task_id, mut seen, sent_end)| async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let Some(task) = st.tasks.get(&task_id).map(|v| v.clone()) else {
                let payload =
                    bytes::Bytes::from("event: error\ndata: {\"error\":\"task_not_found\"}\n\n");
                return Some((
                    Ok::<_, std::convert::Infallible>(payload),
                    (st, task_id, seen, true),
                ));
            };
            if task.version > seen {
                seen = task.version;
                let payload = format!("event: task\ndata: {}\n\n", json!(task));
                return Some((Ok(bytes::Bytes::from(payload)), (st, task_id, seen, false)));
            }
            if is_terminal(&task.status) && !sent_end {
                return Some((
                    Ok(bytes::Bytes::from("event: end\ndata: {\"done\":true}\n\n")),
                    (st, task_id, seen, true),
                ));
            }
            if sent_end {
                return None;
            }
            Some((Ok(bytes::Bytes::from_static(b"")), (st, task_id, seen, false)))
        },
    );
    ([(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")], Body::from_stream(body_stream))
        .into_response()
}

async fn run_task_worker(
    st: AppState,
    task_id: String,
    thread_id: String,
    body: TaskDispatchRequest,
) {
    let Some(task) = bump_task(&st, &task_id, TaskStatus::Running, None, None) else {
        return;
    };
    post_webhook(&st, &task).await;
    let langgraph_url = st.langgraph_url.read().await.clone();
    let run_url = if body.stream {
        format!("{}/threads/{thread_id}/runs/stream", langgraph_url.trim_end_matches('/'))
    } else {
        format!("{}/threads/{thread_id}/runs", langgraph_url.trim_end_matches('/'))
    };
    let run_req = json!({
        "input": body.input,
        "config": {
            "configurable": body.configurable.unwrap_or_else(|| json!({}))
        },
        "stream_mode": ["values", "messages-tuple", "end", "error"]
    });
    if body.stream {
        match st.http_client.post(run_url).json(&run_req).send().await {
            Ok(resp) if resp.status().is_success() => {
                let mut stream = resp.bytes_stream();
                let mut total = 0_usize;
                while let Some(next) = futures::StreamExt::next(&mut stream).await {
                    match next {
                        Ok(chunk) => {
                            if total >= MAX_STREAM_CHUNKS {
                                let failed = bump_task(
                                    &st,
                                    &task_id,
                                    TaskStatus::Failed,
                                    None,
                                    Some(format!(
                                        "stream output exceeded chunk limit {}",
                                        MAX_STREAM_CHUNKS
                                    )),
                                );
                                if let Some(task) = failed {
                                    metrics::counter!("open_harness_manage_task_failed_total")
                                        .increment(1);
                                    post_webhook(&st, &task).await;
                                }
                                return;
                            }
                            let payload = String::from_utf8_lossy(&chunk).to_string();
                            let _ =
                                bump_task(&st, &task_id, TaskStatus::Running, Some(payload), None);
                            total += 1;
                        }
                        Err(err) => {
                            let failed = bump_task(
                                &st,
                                &task_id,
                                TaskStatus::Failed,
                                None,
                                Some(format!("stream read failed: {err}")),
                            );
                            if let Some(task) = failed {
                                metrics::counter!("open_harness_manage_task_failed_total")
                                    .increment(1);
                                post_webhook(&st, &task).await;
                            }
                            return;
                        }
                    }
                }
                if let Some(done) = bump_task(&st, &task_id, TaskStatus::Completed, None, None) {
                    metrics::counter!("open_harness_manage_task_completed_total").increment(1);
                    post_webhook(&st, &done).await;
                }
            }
            Ok(resp) => {
                let failed = bump_task(
                    &st,
                    &task_id,
                    TaskStatus::Failed,
                    None,
                    Some(format!("upstream status {}", resp.status())),
                );
                if let Some(task) = failed {
                    metrics::counter!("open_harness_manage_task_failed_total").increment(1);
                    post_webhook(&st, &task).await;
                }
            }
            Err(err) => {
                let failed = bump_task(
                    &st,
                    &task_id,
                    TaskStatus::Failed,
                    None,
                    Some(format!("upstream error {err}")),
                );
                if let Some(task) = failed {
                    metrics::counter!("open_harness_manage_task_failed_total").increment(1);
                    post_webhook(&st, &task).await;
                }
            }
        }
        return;
    }

    match st.http_client.post(run_url).json(&run_req).send().await {
        Ok(resp) if resp.status().is_success() => {
            let output = resp.text().await.unwrap_or_default();
            if let Some(done) = bump_task(&st, &task_id, TaskStatus::Completed, Some(output), None)
            {
                metrics::counter!("open_harness_manage_task_completed_total").increment(1);
                post_webhook(&st, &done).await;
            }
        }
        Ok(resp) => {
            let failed = bump_task(
                &st,
                &task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream status {}", resp.status())),
            );
            if let Some(task) = failed {
                metrics::counter!("open_harness_manage_task_failed_total").increment(1);
                post_webhook(&st, &task).await;
            }
        }
        Err(err) => {
            let failed = bump_task(
                &st,
                &task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream error {err}")),
            );
            if let Some(task) = failed {
                metrics::counter!("open_harness_manage_task_failed_total").increment(1);
                post_webhook(&st, &task).await;
            }
        }
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
async fn storage_switch(
    State(st): State<AppState>,
    Json(body): Json<StorageSwitch>,
) -> impl IntoResponse {
    let backend = StorageBackendKind::from_mode(&body.backend);
    let applied = matches!(
        backend,
        StorageBackendKind::LocalFs
            | StorageBackendKind::Sqlite
            | StorageBackendKind::Postgres
            | StorageBackendKind::Redis
            | StorageBackendKind::S3
    );
    if applied {
        let mut s = st.store.write().await;
        s.storage_mode = body.backend.clone();
    }
    (StatusCode::OK, Json(StorageSwitchResponse { backend: body.backend, applied }))
}

async fn reload_config(State(st): State<AppState>) -> impl IntoResponse {
    match reload_cached() {
        Ok(cfg) => {
            *st.langgraph_url.write().await = cfg.manage.langgraph_url.clone();
            update_settings(
                &st.auth_state,
                auth_settings_from_config(
                    cfg.manage.auth.enabled,
                    cfg.manage.auth.api_keys.clone(),
                    cfg.manage.auth.bearer_tokens.clone(),
                ),
            )
            .await;
            let mut store = st.store.write().await;
            store.models = cfg
                .models
                .iter()
                .map(|m| ModelInfo {
                    name: m.name.clone(),
                    model: m.model.clone(),
                    display_name: m.display_name.clone(),
                    description: format!("provider: {}", m.use_provider),
                    supports_thinking: false,
                    supports_reasoning_effort: false,
                })
                .collect();
            store.storage_mode = cfg.storage.mode;
            (StatusCode::OK, Json(json!({"reloaded": true}))).into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"reloaded": false, "error": err.to_string()})),
        )
            .into_response(),
    }
}

async fn list_models(State(st): State<AppState>) -> impl IntoResponse {
    let s = st.store.read().await;
    Json(json!({ "models": s.models }))
}

async fn get_model(
    State(st): State<AppState>,
    Path(model_name): Path<String>,
) -> impl IntoResponse {
    let s = st.store.read().await;
    if let Some(m) = s.models.iter().find(|m| m.name == model_name) {
        (StatusCode::OK, Json(json!(m))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error":"model_not_found"}))).into_response()
    }
}

async fn get_mcp_config(State(st): State<AppState>) -> impl IntoResponse {
    let s = st.store.read().await;
    Json(json!({ "mcp_servers": s.mcp_servers }))
}

async fn put_mcp_config(
    State(st): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut s = st.store.write().await;
    s.mcp_servers = body.get("mcp_servers").cloned().unwrap_or_else(|| json!({}));
    let _ = persist_json(&st.local_fs_root.join("config").join("mcp_servers.json"), &s.mcp_servers);
    Json(json!({ "mcp_servers": s.mcp_servers }))
}

async fn get_memory(State(st): State<AppState>) -> impl IntoResponse {
    let mut thread_facts = serde_json::Map::new();
    if let Ok(mut rd) = tokio::fs::read_dir(st.local_fs_root.join("memory")).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let name = ent.file_name();
            let Some(filename) = name.to_str() else {
                continue;
            };
            let Some(id) = filename.strip_suffix(".json") else {
                continue;
            };
            let Ok(thread_id) = Uuid::parse_str(id) else {
                continue;
            };
            let facts = st.storage.list_facts(thread_id).await.unwrap_or_default();
            thread_facts.insert(id.to_string(), json!(facts));
        }
    }
    Json(json!({ "facts": thread_facts }))
}

async fn reload_memory() -> impl IntoResponse {
    Json(json!({ "reloaded": true }))
}

async fn get_memory_config() -> impl IntoResponse {
    Json(json!({
        "enabled": true,
        "storage_path": ".deer-flow/local-fs/memory",
        "debounce_seconds": 2
    }))
}

async fn get_memory_status(State(st): State<AppState>) -> impl IntoResponse {
    let mut facts_count = 0usize;
    if let Ok(mut rd) = tokio::fs::read_dir(st.local_fs_root.join("memory")).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let Some(filename) = ent.file_name().to_str().map(ToString::to_string) else {
                continue;
            };
            let Some(id) = filename.strip_suffix(".json") else {
                continue;
            };
            if let Ok(thread_id) = Uuid::parse_str(id) {
                facts_count += st.storage.list_facts(thread_id).await.unwrap_or_default().len();
            }
        }
    }
    Json(json!({
        "config": {"enabled": true},
        "data": {"facts_count": facts_count}
    }))
}

async fn list_skills(State(st): State<AppState>) -> impl IntoResponse {
    let skills = st.storage.list_skills().await.unwrap_or_default();
    let skills: Vec<serde_json::Value> =
        skills.into_iter().map(|s| json!({"name": s.name, "enabled": s.enabled})).collect();
    Json(json!({ "skills": skills }))
}

async fn get_skill(
    State(st): State<AppState>,
    Path(skill_name): Path<String>,
) -> impl IntoResponse {
    match st.storage.get_skill(&skill_name).await {
        Ok(Some(skill)) => {
            (StatusCode::OK, Json(json!({"name": skill.name, "enabled": skill.enabled})))
                .into_response()
        }
        Ok(None) => {
            (StatusCode::NOT_FOUND, Json(json!({"error":"skill_not_found"}))).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"skill_store_failure"})))
            .into_response(),
    }
}

async fn update_skill(
    State(st): State<AppState>,
    Path(skill_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    if st.storage.put_skill(&SkillRecord { name: skill_name.clone(), enabled }).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"persist_skill_failed"})))
            .into_response();
    }
    Json(json!({"name": skill_name, "enabled": enabled})).into_response()
}

async fn seed_default_skills(storage: &LocalFsStateStore) {
    let defaults = [("research", true), ("report-generation", true)];
    for (name, enabled) in defaults {
        let existing = storage.get_skill(name).await.ok().flatten();
        if existing.is_none() {
            let _ = storage.put_skill(&SkillRecord { name: name.to_string(), enabled }).await;
        }
    }
}

async fn init_memory_defaults(storage: &LocalFsStateStore) {
    let demo_thread = Uuid::nil();
    let facts = storage.list_facts(demo_thread).await.unwrap_or_default();
    if facts.is_empty() {
        let _ = storage.append_fact(demo_thread, "system:memory_initialized").await;
    }
}

async fn bootstrap_storage(state: &AppState) {
    seed_default_skills(&state.storage).await;
    init_memory_defaults(&state.storage).await;
}

async fn upload_thread_files(
    State(st): State<AppState>,
    Path(thread_id): Path<String>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let base = st.threads_root.join(thread_id).join("uploads");
    if tokio::fs::create_dir_all(&base).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"create_dir"})))
            .into_response();
    }
    let mut files = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("upload-{}.bin", Uuid::new_v4()));
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(_) => continue,
        };
        let path = base.join(&filename);
        if tokio::fs::write(path, bytes).await.is_ok() {
            files.push(filename);
        }
    }
    (StatusCode::OK, Json(json!({"success": true, "files": files}))).into_response()
}

async fn list_thread_uploads(
    State(st): State<AppState>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    let base = st.threads_root.join(thread_id).join("uploads");
    let mut out = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(base).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    Json(json!({"files": out}))
}

async fn delete_thread_upload(
    State(st): State<AppState>,
    Path((thread_id, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    let path = st.threads_root.join(thread_id).join("uploads").join(filename);
    match tokio::fs::remove_file(path).await {
        Ok(_) => (StatusCode::NO_CONTENT, "").into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ArtifactQuery {
    download: Option<bool>,
}

async fn get_thread_artifact(
    State(st): State<AppState>,
    Path((thread_id, path)): Path<(String, String)>,
    Query(query): Query<ArtifactQuery>,
) -> impl IntoResponse {
    let full = st.threads_root.join(thread_id).join("artifacts").join(path);
    let bytes = match tokio::fs::read(&full).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    let content_type = "application/octet-stream";
    let mut resp = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type);
    if query.download.unwrap_or(false) {
        resp = resp.header("content-disposition", "attachment");
    }
    resp.body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build").into_response())
}

async fn post_suggestions(
    State(st): State<AppState>,
    Path(thread_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut suggestions = Vec::new();
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        if let Some(last) = messages.last().and_then(|m| m.get("content")).and_then(|v| v.as_str())
        {
            suggestions.push(format!("请展开：{}", last.chars().take(24).collect::<String>()));
            suggestions.push("请给出实现步骤".to_string());
        }
    }
    if suggestions.is_empty() {
        suggestions.push("请继续".to_string());
    }
    let task_path = st.local_fs_root.join("tasks").join(format!("{}.json", thread_id));
    let _ = persist_json(&task_path, &json!({"thread_id": thread_id, "suggestions": suggestions}));
    Json(json!({"suggestions": suggestions}))
}

fn persist_json(path: &PathBuf, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap_or_default())
}

async fn list_agents(State(st): State<AppState>) -> impl IntoResponse {
    let s = st.store.read().await;
    Json(json!({"agents": s.agents.values().cloned().collect::<Vec<_>>()}))
}

async fn check_agent_name(
    State(st): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let name = query.get("name").cloned().unwrap_or_default();
    let s = st.store.read().await;
    Json(json!({"available": !s.agents.contains_key(&name), "name": name}))
}

async fn get_agent(State(st): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    let s = st.store.read().await;
    if let Some(agent) = s.agents.get(&name) {
        (StatusCode::OK, Json(agent.clone())).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error":"agent_not_found"}))).into_response()
    }
}

async fn create_agent(
    State(st): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed").to_string();
    let mut s = st.store.write().await;
    s.agents.insert(name, body.clone());
    (StatusCode::CREATED, Json(body))
}

async fn update_agent(
    State(st): State<AppState>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut s = st.store.write().await;
    s.agents.insert(name, body.clone());
    Json(body)
}

async fn delete_agent(State(st): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    let mut s = st.store.write().await;
    s.agents.remove(&name);
    StatusCode::NO_CONTENT
}

async fn get_user_profile(State(st): State<AppState>) -> impl IntoResponse {
    let path = st.threads_root.join("USER.md");
    let content = tokio::fs::read_to_string(path).await.ok();
    Json(json!({ "content": content }))
}

async fn put_user_profile(
    State(st): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let path = st.threads_root.join("USER.md");
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let _ = tokio::fs::write(path, content).await;
    Json(json!({ "content": content }))
}

async fn get_channels(State(st): State<AppState>) -> impl IntoResponse {
    let s = st.store.read().await;
    Json(json!({ "channels": s.channels, "storage_mode": s.storage_mode }))
}

async fn restart_channel(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let mut s = st.store.write().await;
    s.channels.insert(name.clone(), "running".to_string());
    Json(json!({"name": name, "status": "restarted"}))
}
