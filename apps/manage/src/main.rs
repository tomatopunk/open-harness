use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config_runtime::load_or_default;
use metrics_exporter_prometheus::PrometheusBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use state_abstraction::{
    LocalFsStateStore, MemoryStore, SkillRecord, SkillStore, StorageBackendKind,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

mod thread_delete;

use thread_delete::ThreadDeleteEngine;

#[derive(Clone)]
struct AppState {
    delete_engine: Arc<ThreadDeleteEngine>,
    threads_root: PathBuf,
    local_fs_root: PathBuf,
    store: Arc<RwLock<ManageStore>>,
    storage: Arc<LocalFsStateStore>,
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

    let cfg = load_or_default();
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

    let mut channels = HashMap::new();
    channels.insert("dingtalk".to_string(), "running".to_string());
    channels.insert("wecom".to_string(), "running".to_string());
    let storage = Arc::new(LocalFsStateStore::new(local_fs_root.clone()));

    let state = AppState {
        delete_engine,
        threads_root: threads_root.clone(),
        local_fs_root: local_fs_root.clone(),
        storage: storage.clone(),
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
