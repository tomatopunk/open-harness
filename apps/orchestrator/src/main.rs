use axum::{routing::get, routing::post, Json, Router};
use config_runtime::load_or_default;
use orchestrator_core::LeadPipeline;
use protocol_compat::Configurable;
use runtime_kernel::RuntimeEvent;
use runtime_langgraph_adapter::LanggraphAdapter;
use runtime_llm_chain_adapter::LlmChainAdapter;
use serde::Deserialize;
use serde_json::json;
use state_abstraction::{
    LocalFsLayout, LocalFsStateStore, MemoryStore, SandboxExecution, SandboxExecutionStore,
    SkillRecord, SkillStore, SubagentTask, SubagentTaskStore, ToolRecord, ToolRecordStore,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    store: Arc<LocalFsStateStore>,
    runtime_engine: RuntimeEngine,
}

#[derive(Clone, Copy)]
enum RuntimeEngine {
    LanggraphCompatible,
    LlmChain,
}

impl RuntimeEngine {
    fn from_str(value: &str) -> Self {
        match value {
            "llm-chain" => Self::LlmChain,
            _ => Self::LanggraphCompatible,
        }
    }
}

async fn run_runtime(
    engine: RuntimeEngine,
    configurable: Configurable,
    messages: Vec<serde_json::Value>,
) -> Vec<RuntimeEvent> {
    match engine {
        RuntimeEngine::LanggraphCompatible => {
            let adapter = LanggraphAdapter::default();
            adapter
                .run(configurable, messages)
                .await
                .unwrap_or_else(|e| vec![RuntimeEvent::Error { message: e.to_string() }])
        }
        RuntimeEngine::LlmChain => {
            let adapter = LlmChainAdapter::default();
            adapter
                .run(configurable, messages)
                .await
                .unwrap_or_else(|e| vec![RuntimeEvent::Error { message: e.to_string() }])
        }
    }
}

async fn pipeline_check(
    axum::extract::State(st): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx = pipeline.prepare(Configurable::default()).await.expect("pipeline");
    let events = run_runtime(st.runtime_engine, Configurable::default(), vec![]).await;
    Json(json!({
        "middleware": "ok",
        "configurable": ctx.configurable,
        "token_usage_estimate": ctx.token_usage_estimate,
        "runtime_engine": match st.runtime_engine {
            RuntimeEngine::LanggraphCompatible => "langgraph-compatible",
            RuntimeEngine::LlmChain => "llm-chain"
        },
        "events_count": events.len()
    }))
}

#[derive(Debug, Deserialize)]
struct OrchestrateRequest {
    #[serde(default)]
    configurable: Configurable,
    #[serde(default)]
    messages: Vec<serde_json::Value>,
}

async fn run_orchestrate_with_state(
    axum::extract::State(st): axum::extract::State<AppState>,
    Json(body): Json<OrchestrateRequest>,
) -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx =
        match pipeline.prepare_with_input(body.configurable.clone(), body.messages.clone()).await {
            Ok(ctx) => ctx,
            Err(_) => pipeline.prepare(body.configurable.clone()).await.expect("pipeline"),
        };

    let thread_id = body
        .configurable
        .thread_id
        .as_deref()
        .and_then(|v| Uuid::parse_str(v).ok())
        .unwrap_or_else(Uuid::new_v4);

    for fact in &ctx.memory_facts {
        let _ = st.store.append_fact(thread_id, fact).await;
    }

    let _ = st
        .store
        .put_skill(&SkillRecord {
            name: "research".to_string(),
            enabled: body.configurable.skills_enabled.unwrap_or(true),
        })
        .await;

    let _ = st
        .store
        .append_tool_record(&ToolRecord {
            thread_id,
            tool_name: "orchestrate.prepare".to_string(),
            args: json!({"messages_count": body.messages.len()}),
            result: json!({"loop_detected": ctx.loop_detected, "todos": ctx.todos}),
            created_at: chrono::Utc::now(),
        })
        .await;

    if body.configurable.sandbox_enabled.unwrap_or(false) {
        let _ = st
            .store
            .append_execution(&SandboxExecution {
                execution_id: Uuid::new_v4(),
                thread_id,
                command: "prepare_runtime".to_string(),
                exit_code: 0,
                stdout: "sandbox simulated".to_string(),
                stderr: String::new(),
                created_at: chrono::Utc::now(),
            })
            .await;
    }

    if body.configurable.subagent_enabled.unwrap_or(false) {
        let max_subagents = body.configurable.max_concurrent_subagents.unwrap_or(1);
        let task = SubagentTask {
            task_id: Uuid::new_v4(),
            thread_id,
            agent_name: "general".to_string(),
            status: "completed".to_string(),
            input: json!({"max_concurrent_subagents": max_subagents}),
            output: Some(json!({"result":"ok"})),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let _ = st.store.upsert_task(&task).await;
    }

    let facts = st.store.list_facts(thread_id).await.unwrap_or_default();
    let tool_records = st.store.list_tool_records(thread_id).await.unwrap_or_default();
    let subagent_records = st.store.list_tasks_by_thread(thread_id).await.unwrap_or_default();
    let sandbox_records = st.store.list_executions(thread_id).await.unwrap_or_default();
    let skills = st.store.list_skills().await.unwrap_or_default();
    let events =
        run_runtime(st.runtime_engine, body.configurable.clone(), body.messages.clone()).await;

    Json(json!({
        "ok": true,
        "thread_id": thread_id,
        "loop_detected": ctx.loop_detected,
        "token_usage_estimate": ctx.token_usage_estimate,
        "todos": ctx.todos,
        "memory_facts": facts,
        "skills": skills,
        "tool_records_count": tool_records.len(),
        "subagent_tasks_count": subagent_records.len(),
        "sandbox_executions_count": sandbox_records.len(),
        "events": events
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
    let store = Arc::new(LocalFsStateStore::new(&cfg.storage.local_fs_root));
    let app_state =
        AppState { store, runtime_engine: RuntimeEngine::from_str(&cfg.runtime.engine) };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/openapi.json", get(|| async {
            Json(json!({
                "openapi": "3.1.0",
                "info": {"title": "open-harness-orchestrator", "version": "0.1.0"},
                "paths": {
                    "/healthz": {"get": {"summary": "Health check"}},
                    "/internal/pipeline-check": {"post": {"summary": "Check runtime pipeline"}},
                    "/internal/orchestrate": {"post": {"summary": "Run orchestration and persist state"}}
                }
            }))
        }))
        .route("/internal/pipeline-check", post(pipeline_check))
        .route("/internal/orchestrate", post(run_orchestrate_with_state))
        .layer(TraceLayer::new_for_http());
    let app = app.with_state(app_state);

    let addr: SocketAddr = "0.0.0.0:8083".parse()?;
    tracing::info!("open-harness-orchestrator on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
