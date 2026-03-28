use app_auth::{
    build_settings, require_auth, shared_state, update_settings, AuthSettings, SharedAuthState,
};
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use channel_bootstrap::configured_channels;
use config_runtime::{load_cached_or_default, reload_cached};
use governance_plane::GovernanceBundle;
use metrics_exporter_prometheus::PrometheusBuilder;
use runtime_kernel::{McpServerConfig, SkillsRuntime};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_yaml::Value as YamlValue;
use state_abstraction::{
    LocalFsStateStore, ManageTaskStore, MemoryStore, SkillRecord, SkillStore, StorageBackendKind,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use storage_postgres::PostgresManageTaskStore;
use storage_redis::RedisManageTaskStore;
use storage_s3::build_s3_manage_store;
use storage_sqlite::SqliteManageTaskStore;
use tokio::sync::RwLock;
use tokio_util::task::TaskTracker;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

mod security;
mod task_access;
mod task_runtime;
mod tasks;
mod thread_delete;

use security::{sanitize_path_component, sanitize_relative_path, sanitize_thread_id};
use task_runtime::{dispatch_task, get_task, stream_task};
pub(crate) use task_runtime::{TaskRecord, TaskStatus};
use thread_delete::ThreadDeleteEngine;

#[derive(Clone)]
struct AppState {
    delete_engine: Arc<ThreadDeleteEngine>,
    threads_root: PathBuf,
    local_fs_root: PathBuf,
    store: Arc<RwLock<ManageStore>>,
    storage: Arc<LocalFsStateStore>,
    storage_runtime: Arc<RwLock<StorageRuntime>>,
    tasks: Arc<dashmap::DashMap<String, TaskRecord>>,
    task_capacity: usize,
    langgraph_url: Arc<RwLock<String>>,
    http_client: reqwest::Client,
    webhook_secret: Option<String>,
    auth_state: SharedAuthState,
    task_workers: TaskTracker,
    skills_install_dir: PathBuf,
    governance: Arc<GovernanceBundle>,
}

#[derive(Clone)]
struct StorageRuntime {
    active_mode: String,
    manage_tasks: Arc<dyn ManageTaskStore>,
    capabilities: serde_json::Value,
    last_switch_ts: i64,
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

    let langgraph_url = Arc::new(RwLock::new(cfg.manage.langgraph_url.clone()));
    let delete_engine = ThreadDeleteEngine::new(threads_root.clone(), langgraph_url.clone());

    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!("open_harness_manage_requests_total", "Manage API requests");
    metrics::describe_counter!("open_harness_manage_task_created_total", "Manage task created");
    metrics::describe_counter!("open_harness_manage_task_completed_total", "Manage task completed");
    metrics::describe_counter!("open_harness_manage_task_failed_total", "Manage task failed");
    metrics::describe_counter!("open_harness_manage_task_forbidden_total", "Manage task forbidden");
    metrics::describe_counter!(
        "open_harness_manage_invalid_path_total",
        "Manage invalid file path attempts"
    );
    metrics::describe_counter!(
        "open_harness_manage_webhook_success_total",
        "Manage webhook success"
    );
    metrics::describe_counter!(
        "open_harness_manage_webhook_failure_total",
        "Manage webhook failure"
    );
    metrics::describe_counter!("open_harness_manage_shutdown_total", "Manage graceful shutdown");
    metrics::describe_counter!(
        "open_harness_manage_storage_switch_total",
        "Manage storage switch operations"
    );
    metrics::describe_counter!(
        "open_harness_manage_storage_switch_failed_total",
        "Manage storage switch failed operations"
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

    let governance = Arc::new(
        GovernanceBundle::load_from_dir(&cfg.runtime.governance_root).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "governance load failed; using defaults");
            GovernanceBundle::default()
        }),
    );

    let state = AppState {
        delete_engine,
        threads_root: threads_root.clone(),
        local_fs_root: local_fs_root.clone(),
        storage: storage.clone(),
        storage_runtime: Arc::new(RwLock::new(StorageRuntime {
            active_mode: cfg.storage.mode.clone(),
            manage_tasks: storage.clone(),
            capabilities: storage_capabilities("local_fs"),
            last_switch_ts: now_ts(),
        })),
        tasks: Arc::new(dashmap::DashMap::new()),
        task_capacity: 1000,
        langgraph_url,
        http_client: reqwest::Client::new(),
        webhook_secret: cfg.manage.webhook_secret.clone(),
        auth_state: auth_state.clone(),
        task_workers: TaskTracker::new(),
        skills_install_dir: local_fs_root.join("skills"),
        governance,
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

    let task_workers = state.task_workers.clone();
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/openapi.json", get(openapi_spec))
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
        .route("/api/mcp/oauth/status", get(get_mcp_oauth_status))
        .route("/api/memory", get(get_memory))
        .route("/api/memory/reload", post(reload_memory))
        .route("/api/memory/config", get(get_memory_config))
        .route("/api/memory/status", get(get_memory_status))
        .route("/api/skills", get(list_skills))
        .route("/api/skills/install", post(install_skill_archive))
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
        .route("/api/manage/admin/storage/status", get(storage_status))
        .route("/api/manage/admin/config/reload", post(reload_config))
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_auth))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.manage.bind).await?;
    tracing::info!("open-harness-manage listening on {}", cfg.manage.bind);
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal(task_workers)).await?;
    Ok(())
}

async fn shutdown_signal(task_workers: TaskTracker) {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received, waiting for task workers");
    task_workers.close();
    task_workers.wait().await;
    metrics::counter!("open_harness_manage_shutdown_total").increment(1);
}

async fn health(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "policy_version": st.governance.policy_version,
    }))
}

async fn openapi_spec() -> impl IntoResponse {
    Json(json!({
        "openapi": "3.1.0",
        "info": {"title": "open-harness-manage", "version": "0.1.0"},
        "paths": {
            "/healthz": {"get": {"summary": "Health check"}},
            "/api/models": {"get": {"summary": "List models"}},
            "/api/mcp/config": {"get": {"summary": "Get MCP config"}, "put": {"summary": "Update MCP config"}},
            "/api/mcp/oauth/status": {"get": {"summary": "Get MCP OAuth readiness"}},
            "/api/memory": {"get": {"summary": "Get memory"}},
            "/api/skills": {"get": {"summary": "List skills"}},
            "/api/skills/install": {"post": {"summary": "Install skill archive metadata"}},
            "/api/threads/{thread_id}": {"delete": {"summary": "Delete thread"}},
            "/api/manage/admin/storage/switch": {"post": {"summary": "Switch storage backend"}},
            "/api/manage/admin/storage/status": {"get": {"summary": "Get runtime storage backend status"}}
        }
    }))
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

fn sqlite_connect_url(cfg: &config_runtime::AppConfig) -> Option<String> {
    let u = cfg.storage.sqlite_url.clone().or(cfg.manage.sqlite_url.clone())?;
    Some(if u.starts_with("sqlite:") {
        u
    } else {
        format!("sqlite://{}", u.trim_start_matches('/'))
    })
}

#[derive(Debug, Deserialize)]
struct StorageSwitch {
    backend: String,
}

#[derive(Debug, Serialize)]
struct StorageSwitchResponse {
    requested_backend: String,
    active_backend: String,
    applied: bool,
    persisted: bool,
    capabilities: serde_json::Value,
    reason: Option<String>,
}

async fn storage_switch(
    State(st): State<AppState>,
    Json(body): Json<StorageSwitch>,
) -> impl IntoResponse {
    metrics::counter!("open_harness_manage_storage_switch_total").increment(1);
    let requested_backend = body.backend;
    let backend = StorageBackendKind::from_mode(&requested_backend);
    let (applied, active_backend, capabilities, reason) =
        match switch_runtime_storage(&st, backend).await {
            Ok(out) => out,
            Err(msg) => {
                metrics::counter!("open_harness_manage_storage_switch_failed_total").increment(1);
                let rt = st.storage_runtime.read().await;
                (false, rt.active_mode.clone(), rt.capabilities.clone(), Some(msg))
            }
        };

    let persisted =
        if applied { persist_storage_mode_to_config(&active_backend).is_ok() } else { false };

    if applied {
        let mut s = st.store.write().await;
        s.storage_mode = active_backend.clone();
    }
    (
        StatusCode::OK,
        Json(StorageSwitchResponse {
            requested_backend,
            active_backend,
            applied,
            persisted,
            capabilities,
            reason,
        }),
    )
}

async fn switch_runtime_storage(
    st: &AppState,
    backend: StorageBackendKind,
) -> Result<(bool, String, serde_json::Value, Option<String>), String> {
    match backend {
        StorageBackendKind::LocalFs => {
            let mut rt = st.storage_runtime.write().await;
            rt.active_mode = "local_fs".to_string();
            rt.manage_tasks = st.storage.clone();
            rt.capabilities = storage_capabilities("local_fs");
            rt.last_switch_ts = now_ts();
            Ok((true, rt.active_mode.clone(), rt.capabilities.clone(), None))
        }
        StorageBackendKind::Postgres => {
            let cfg = load_cached_or_default();
            let postgres_url =
                cfg.storage.postgres_url.or(cfg.manage.postgres_url).ok_or_else(|| {
                    "postgres_url is missing in storage/manage config".to_string()
                })?;
            let pg = PostgresManageTaskStore::connect(&postgres_url)
                .await
                .map_err(|e| format!("postgres connect failed: {e}"))?;
            let mut rt = st.storage_runtime.write().await;
            rt.active_mode = "postgres".to_string();
            rt.manage_tasks = Arc::new(pg);
            rt.capabilities = storage_capabilities("postgres");
            rt.last_switch_ts = now_ts();
            Ok((true, rt.active_mode.clone(), rt.capabilities.clone(), None))
        }
        StorageBackendKind::Sqlite => {
            let cfg = load_cached_or_default();
            let url = sqlite_connect_url(&cfg)
                .ok_or_else(|| "sqlite_url is missing in storage/manage config".to_string())?;
            let mut rt = st.storage_runtime.write().await;
            rt.active_mode = "sqlite".to_string();
            rt.capabilities = storage_capabilities("sqlite");
            rt.last_switch_ts = now_ts();
            match SqliteManageTaskStore::connect(&url).await {
                Ok(sqlite) => {
                    rt.manage_tasks = Arc::new(sqlite);
                    Ok((true, rt.active_mode.clone(), rt.capabilities.clone(), None))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "sqlite manage_tasks unavailable; fallback local_fs");
                    rt.manage_tasks = st.storage.clone();
                    Ok((
                        true,
                        rt.active_mode.clone(),
                        rt.capabilities.clone(),
                        Some(format!(
                            "sqlite connect failed ({e}); using local_fs manage_tasks (deterministic fallback)"
                        )),
                    ))
                }
            }
        }
        StorageBackendKind::Redis => {
            let cfg = load_cached_or_default();
            let redis_url = cfg
                .storage
                .redis_url
                .clone()
                .ok_or_else(|| "redis_url is missing in storage config".to_string())?;
            let mut rt = st.storage_runtime.write().await;
            rt.active_mode = "redis".to_string();
            rt.capabilities = storage_capabilities("redis");
            rt.last_switch_ts = now_ts();
            match RedisManageTaskStore::connect(&redis_url).await {
                Ok(redis_store) => {
                    rt.manage_tasks = Arc::new(redis_store);
                    Ok((true, rt.active_mode.clone(), rt.capabilities.clone(), None))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "redis manage_tasks unavailable; fallback local_fs");
                    rt.manage_tasks = st.storage.clone();
                    Ok((
                        true,
                        rt.active_mode.clone(),
                        rt.capabilities.clone(),
                        Some(format!(
                            "redis connect failed ({e}); using local_fs manage_tasks (deterministic fallback)"
                        )),
                    ))
                }
            }
        }
        StorageBackendKind::S3 => {
            let cfg = load_cached_or_default();
            let bucket = cfg
                .storage
                .s3_bucket
                .clone()
                .ok_or_else(|| "s3_bucket is missing in storage config".to_string())?;
            let prefix = cfg.storage.s3_prefix.clone();
            let mut rt = st.storage_runtime.write().await;
            rt.active_mode = "s3".to_string();
            rt.capabilities = storage_capabilities("s3");
            rt.last_switch_ts = now_ts();
            match build_s3_manage_store(&bucket, prefix) {
                Ok(s3_store) => {
                    rt.manage_tasks = Arc::new(s3_store);
                    Ok((true, rt.active_mode.clone(), rt.capabilities.clone(), None))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "s3 manage_tasks unavailable; fallback local_fs");
                    rt.manage_tasks = st.storage.clone();
                    Ok((
                        true,
                        rt.active_mode.clone(),
                        rt.capabilities.clone(),
                        Some(format!(
                            "s3 init failed ({e}); using local_fs manage_tasks (deterministic fallback)"
                        )),
                    ))
                }
            }
        }
    }
}

fn storage_capabilities(mode: &str) -> serde_json::Value {
    match mode {
        "postgres" => json!({
            "manage_tasks": true,
            "memory": false,
            "skills": false,
            "tool_records": false,
            "subagent_tasks": false,
            "sandbox_logs": false
        }),
        "sqlite" => json!({
            "manage_tasks": true,
            "memory": true,
            "skills": true,
            "tool_records": true,
            "subagent_tasks": true,
            "sandbox_logs": true
        }),
        "redis" => json!({
            "manage_tasks": true,
            "memory": true,
            "skills": true,
            "tool_records": false,
            "subagent_tasks": false,
            "sandbox_logs": false
        }),
        "s3" => json!({
            "manage_tasks": true,
            "memory": false,
            "skills": true,
            "tool_records": false,
            "subagent_tasks": false,
            "sandbox_logs": true
        }),
        "local_fs" => json!({
            "manage_tasks": true,
            "memory": true,
            "skills": true,
            "tool_records": true,
            "subagent_tasks": true,
            "sandbox_logs": true
        }),
        _ => json!({
            "manage_tasks": false,
            "memory": false,
            "skills": false,
            "tool_records": false,
            "subagent_tasks": false,
            "sandbox_logs": false
        }),
    }
}

#[derive(Debug, Serialize)]
struct StorageStatusResponse {
    active_backend: String,
    capabilities: serde_json::Value,
    last_switch_ts: i64,
}

async fn storage_status(State(st): State<AppState>) -> impl IntoResponse {
    let rt = st.storage_runtime.read().await;
    (
        StatusCode::OK,
        Json(StorageStatusResponse {
            active_backend: rt.active_mode.clone(),
            capabilities: rt.capabilities.clone(),
            last_switch_ts: rt.last_switch_ts,
        }),
    )
}

fn config_path() -> PathBuf {
    std::env::var("OPEN_HARNESS_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config.yaml"))
}

fn persist_storage_mode_to_config(mode: &str) -> std::io::Result<()> {
    let path = config_path();
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| "---\n".to_string());
    let mut root: YamlValue =
        serde_yaml::from_str(&content).unwrap_or_else(|_| YamlValue::Mapping(Default::default()));
    if !matches!(root, YamlValue::Mapping(_)) {
        root = YamlValue::Mapping(Default::default());
    }
    let storage_key = YamlValue::String("storage".to_string());
    let mode_key = YamlValue::String("mode".to_string());
    if let YamlValue::Mapping(map) = &mut root {
        let storage_entry =
            map.entry(storage_key).or_insert_with(|| YamlValue::Mapping(Default::default()));
        if !matches!(storage_entry, YamlValue::Mapping(_)) {
            *storage_entry = YamlValue::Mapping(Default::default());
        }
        if let YamlValue::Mapping(storage_map) = storage_entry {
            storage_map.insert(mode_key, YamlValue::String(mode.to_string()));
        }
    }
    let serialized = serde_yaml::to_string(&root).unwrap_or_default();
    std::fs::write(path, serialized)
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
            let storage_mode = cfg.storage.mode.clone();
            store.storage_mode = storage_mode.clone();
            drop(store);
            let backend = StorageBackendKind::from_mode(&storage_mode);
            let switch_result = switch_runtime_storage(&st, backend).await;
            let runtime_switched = switch_result.is_ok();
            if !runtime_switched {
                metrics::counter!("open_harness_manage_storage_switch_failed_total").increment(1);
            }
            (
                StatusCode::OK,
                Json(json!({"reloaded": true, "runtime_storage_switched": runtime_switched})),
            )
                .into_response()
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

async fn get_mcp_oauth_status(State(st): State<AppState>) -> impl IntoResponse {
    let s = st.store.read().await;
    let servers = s.mcp_servers.as_object().cloned().unwrap_or_default();
    let mut status = Vec::new();
    for (name, cfg) in servers {
        let mut full_cfg = cfg;
        if full_cfg.get("name").is_none() {
            full_cfg["name"] = json!(name.clone());
        }
        let parsed: Option<McpServerConfig> = serde_json::from_value(full_cfg).ok();
        let oauth_enabled = parsed.as_ref().map(McpServerConfig::oauth_enabled).unwrap_or(false);
        status.push(json!({"name": name, "oauth_enabled": oauth_enabled}));
    }
    Json(json!({ "servers": status }))
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

#[derive(Debug, Deserialize)]
struct InstallSkillRequest {
    archive_name: String,
    #[serde(default)]
    enabled: Option<bool>,
}

async fn install_skill_archive(
    State(st): State<AppState>,
    Json(body): Json<InstallSkillRequest>,
) -> impl IntoResponse {
    if let Err(err) = SkillsRuntime::validate_skill_archive(&body.archive_name) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": err}))).into_response();
    }
    let Some(skill_name) = body.archive_name.strip_suffix(".skill") else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid archive name"})))
            .into_response();
    };
    let enabled = body.enabled.unwrap_or(true);
    let marker = st.skills_install_dir.join(format!("{skill_name}.installed"));
    if let Some(parent) = marker.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"create install dir failed"})),
            )
                .into_response();
        }
    }
    if std::fs::write(&marker, body.archive_name.as_bytes()).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"persist install marker failed"})),
        )
            .into_response();
    }
    if st.storage.put_skill(&SkillRecord { name: skill_name.to_string(), enabled }).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"persist_skill_failed"})))
            .into_response();
    }
    Json(json!({"installed": true, "skill": skill_name, "enabled": enabled})).into_response()
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
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_thread_id"})))
            .into_response();
    };
    let base = st.threads_root.join(thread_id).join("uploads");
    if tokio::fs::create_dir_all(&base).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"create_dir"})))
            .into_response();
    }
    let mut files = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let raw_filename = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("upload-{}.bin", Uuid::new_v4()));
        let Some(filename) = sanitize_path_component(&raw_filename) else {
            metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
            continue;
        };
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
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_thread_id"})))
            .into_response();
    };
    let base = st.threads_root.join(thread_id).join("uploads");
    let mut out = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(base).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    Json(json!({"files": out})).into_response()
}

async fn delete_thread_upload(
    State(st): State<AppState>,
    Path((thread_id, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, "invalid thread_id").into_response();
    };
    let Some(filename) = sanitize_path_component(&filename) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, "invalid filename").into_response();
    };
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
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, "invalid thread_id").into_response();
    };
    let Some(path) = sanitize_relative_path(&path) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, "invalid artifact path").into_response();
    };
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

fn heuristic_suggestions(body: &serde_json::Value) -> Vec<String> {
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
    suggestions
}

fn parse_suggestions_from_completion(v: &serde_json::Value) -> Vec<String> {
    let text = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim();
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(text) {
        return arr.into_iter().filter(|s| !s.is_empty()).take(8).collect();
    }
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if start < end {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(&text[start..=end]) {
                return arr.into_iter().filter(|s| !s.is_empty()).take(8).collect();
            }
        }
    }
    Vec::new()
}

async fn post_suggestions(
    State(st): State<AppState>,
    Path(thread_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_thread_id"})))
            .into_response();
    };
    let cfg = load_cached_or_default();
    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(4).min(8);
    let model_name = body
        .get("model_name")
        .and_then(|v| v.as_str())
        .or_else(|| cfg.models.first().map(|m| m.name.as_str()))
        .unwrap_or("gpt-4");

    let m = cfg.models.iter().find(|model| model.name == model_name).or_else(|| cfg.models.first());

    let mut suggestions = Vec::new();
    if let Some(model_cfg) = m {
        let api_key = model_cfg
            .api_key
            .as_ref()
            .map(|s| config_runtime::resolve_env_var_ref(s))
            .filter(|s| !s.is_empty());
        if let Some(key) = api_key {
            let base = model_cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let url = if base.contains("/v1") && !base.ends_with("/chat/completions") {
                format!("{}/chat/completions", base.trim_end_matches('/'))
            } else {
                format!("{}/v1/chat/completions", base.trim_end_matches('/'))
            };
            let ctx = body.get("messages").cloned().unwrap_or_else(|| json!([]));
            let sys = "You produce follow-up user suggestions. Reply with ONLY a JSON array of short question strings (same language as the user). No markdown.";
            let user = format!(
                "n={n}. Conversation messages JSON: {}",
                serde_json::to_string(&ctx).unwrap_or_else(|_| "[]".to_string())
            );
            let chat_body = json!({
                "model": model_cfg.model,
                "messages": [
                    {"role": "system", "content": sys},
                    {"role": "user", "content": user}
                ],
                "temperature": 0.5,
                "max_tokens": 400
            });
            if let Ok(resp) = st
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {key}"))
                .header("Content-Type", "application/json")
                .json(&chat_body)
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        suggestions = parse_suggestions_from_completion(&v);
                    }
                }
            }
        }
    }
    if suggestions.is_empty() {
        suggestions = heuristic_suggestions(&body);
    }
    let task_path = st.local_fs_root.join("tasks").join(format!("{}.json", thread_id));
    let _ = persist_json(&task_path, &json!({"thread_id": thread_id, "suggestions": suggestions}));
    Json(json!({"suggestions": suggestions})).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_storage_mode_updates_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("config.yaml");
        std::fs::write(&config_path, "storage:\n  mode: local_fs\nmanage:\n  bind: 0.0.0.0:8081\n")
            .expect("write");
        std::env::set_var("OPEN_HARNESS_CONFIG_PATH", &config_path);
        persist_storage_mode_to_config("sqlite").expect("persist");
        let after = std::fs::read_to_string(&config_path).expect("read");
        assert!(after.contains("mode: sqlite"));
    }

    #[test]
    fn storage_capabilities_are_backend_aware() {
        let local = storage_capabilities("local_fs");
        assert_eq!(local.get("memory").and_then(|v| v.as_bool()), Some(true));
        let pg = storage_capabilities("postgres");
        assert_eq!(pg.get("manage_tasks").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(pg.get("skills").and_then(|v| v.as_bool()), Some(false));
        let sqlite = storage_capabilities("sqlite");
        assert_eq!(sqlite.get("manage_tasks").and_then(|v| v.as_bool()), Some(true));
        let redis = storage_capabilities("redis");
        assert_eq!(redis.get("manage_tasks").and_then(|v| v.as_bool()), Some(true));
        let s3 = storage_capabilities("s3");
        assert_eq!(s3.get("manage_tasks").and_then(|v| v.as_bool()), Some(true));
    }
}
