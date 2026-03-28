use axum::{routing::get, routing::post, Json, Router};
use config_runtime::load_or_default;
use governance_plane::GovernanceBundle;
use graph_runtime_core::GraphRuntime;
use metrics_exporter_prometheus::PrometheusBuilder;
use orchestrator_core::LeadPipeline;
use protocol_compat::Configurable;
use runtime_llm_chain_adapter::{RuntimeRunMetadata, RUNTIME_OUTPUT_SCHEMA_VERSION};
use sandbox_runtime::{LocalSandbox, Sandbox, SandboxRequest};
use serde::Deserialize;
use serde_json::json;
use state_abstraction::{
    CheckpointStore, DynCheckpointStorePort, LocalFsLayout, LocalFsStateStore, MemoryStore,
    SandboxExecution, SandboxExecutionStore, SkillRecord, SkillStore, SubagentTaskStore,
    ToolRecord, ToolRecordStore,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use agent_adapters::{
    default_echo_manifests, DefaultMemoryAdapter, DefaultSkillAdapter, DefaultSubagentAdapter,
    EchoTool, HeuristicLlmAdapter, OpenAiChatLlmAdapter, PersistingToolPort, RegistryToolAdapter,
};
use agent_loop_runtime::{
    run_agent_loop, AgentLoopDeps, AgentLoopRunConfig, RunBudget, ToolLoopConfig,
};
use agent_ports::{LLMPort, ThreadId, ThreadState, TodoItem, ToolPort};
use orchestrator_core::middleware::MiddlewareContext;
use tool_runtime::ToolRegistry;

#[derive(Clone)]
struct AppState {
    store: Arc<LocalFsStateStore>,
    governance: Arc<GovernanceBundle>,
    graph: Arc<GraphRuntime>,
    agent_deps: Arc<AgentLoopDeps>,
}

fn build_agent_loop_deps(
    bundle: &GovernanceBundle,
    store: Arc<LocalFsStateStore>,
) -> AgentLoopDeps {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(EchoTool));
    let manifests = if bundle.tools.manifests.is_empty() {
        default_echo_manifests()
    } else {
        bundle.tools.manifests.clone()
    };
    let manifest_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
    reg.ensure_consistent_with_manifest_list(&manifest_names).unwrap_or_else(|e| {
        panic!("governance tools.yaml / ToolRegistry mismatch: {e}");
    });
    let registry = Arc::new(reg);
    let inner = Arc::new(RegistryToolAdapter::new(registry, manifests));
    let records: Arc<dyn ToolRecordStore> = store.clone();
    let tools: Arc<dyn ToolPort> = Arc::new(PersistingToolPort::new(inner, records));
    let llm: Arc<dyn LLMPort> =
        if std::env::var("OPENAI_API_KEY").map(|s| !s.trim().is_empty()).unwrap_or(false) {
            let model = bundle
                .models
                .default_model
                .clone()
                .or_else(|| bundle.models.entries.first().map(|e| e.name.clone()))
                .unwrap_or_else(|| "gpt-4o-mini".into());
            Arc::new(OpenAiChatLlmAdapter::new(model))
        } else {
            tracing::info!("OPENAI_API_KEY unset; using heuristic LLM adapter (offline)");
            Arc::new(HeuristicLlmAdapter::default())
        };
    let memory = Arc::new(DefaultMemoryAdapter);
    let mut preamble_by_name = HashMap::new();
    for e in &bundle.skills.entries {
        if e.enabled {
            if let Some(p) = &e.preamble {
                preamble_by_name.insert(e.name.clone(), p.clone());
            }
        }
    }
    let skills = Arc::new(DefaultSkillAdapter {
        preamble: "You are the open-harness inner runtime (model-tool-state loop).".into(),
        preamble_by_name,
    });
    let subagents = Arc::new(DefaultSubagentAdapter {
        max_concurrent: bundle.subagents.max_concurrent.max(1) as usize,
    });
    AgentLoopDeps { llm, tools, memory, skills, subagents }
}

fn inner_runtime_metadata(state: &ThreadState, policy_version: &str) -> RuntimeRunMetadata {
    let memory_facts_count: usize = state.memory_commits.iter().map(|m| m.facts.len()).sum();
    let loop_detected = state.governance_marks.tags.iter().any(|t| t == "loop_detected");
    RuntimeRunMetadata {
        schema_version: RUNTIME_OUTPUT_SCHEMA_VERSION,
        engine: "inner".into(),
        loop_detected,
        token_usage_estimate: state.messages.len(),
        tool_calls: state.tool_invocations.len(),
        blocked_tools: Vec::new(),
        warnings: if policy_version.is_empty() {
            Vec::new()
        } else {
            vec![format!("policy_version:{policy_version}")]
        },
        todos_count: state.todos.len(),
        memory_facts_count,
    }
}

async fn pipeline_check(
    axum::extract::State(st): axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    let pipeline = LeadPipeline::default();
    let ctx = pipeline.prepare(Configurable::default()).await.expect("pipeline");
    Json(json!({
        "middleware": "ok",
        "configurable": ctx.configurable,
        "token_usage_estimate": ctx.token_usage_estimate,
        "runtime_engine": "inner",
        "governance": {
            "policy_version": st.governance.policy_version,
        },
        "runtime_metadata": RuntimeRunMetadata {
            schema_version: RUNTIME_OUTPUT_SCHEMA_VERSION,
            engine: "inner".into(),
            loop_detected: false,
            token_usage_estimate: 0,
            tool_calls: 0,
            blocked_tools: Vec::new(),
            warnings: vec![],
            todos_count: 0,
            memory_facts_count: 0,
        },
        "agent_loop": "ready"
    }))
}

fn build_inner_run_config(
    ctx: &MiddlewareContext,
    body: &OrchestrateRequest,
    gov: &GovernanceBundle,
) -> AgentLoopRunConfig {
    let skills_enabled = body.configurable.skills_enabled.unwrap_or(true);
    let enabled_skill_names = if skills_enabled { gov.enabled_skill_names() } else { Vec::new() };
    let seed_todos: Vec<TodoItem> = ctx
        .todos
        .iter()
        .enumerate()
        .map(|(i, title)| TodoItem { id: format!("mw-{i}"), title: title.clone(), done: false })
        .collect();
    AgentLoopRunConfig {
        model_name: body.configurable.model_name.clone(),
        policy_version: gov.policy_version.clone(),
        is_plan_mode: body.configurable.is_plan_mode.unwrap_or(false),
        skills_globally_enabled: skills_enabled,
        enabled_skill_names,
        loop_detected: ctx.loop_detected,
        seed_todos,
    }
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

    let thread_uuid = body
        .configurable
        .thread_id
        .as_deref()
        .and_then(|v| Uuid::parse_str(v).ok())
        .unwrap_or_else(Uuid::new_v4);

    let thread_id = ThreadId::from(thread_uuid);

    for fact in &ctx.memory_facts {
        let _ = st.store.append_fact(thread_uuid, fact).await;
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
            thread_id: thread_uuid,
            tool_name: "orchestrate.prepare".to_string(),
            args: json!({"messages_count": body.messages.len()}),
            result: json!({"loop_detected": ctx.loop_detected, "todos": ctx.todos}),
            created_at: chrono::Utc::now(),
        })
        .await;
    metrics::counter!("open_harness_orchestrator_tool_record_total").increment(1);

    if body.configurable.sandbox_enabled.unwrap_or(false) {
        let sandbox = LocalSandbox;
        let sb_out = sandbox
            .exec(SandboxRequest::new("echo prepare_runtime", std::time::Duration::from_secs(2)))
            .await;
        let (exit_code, stdout, stderr) = match sb_out {
            Ok(out) => (out.exit_code, out.stdout, out.stderr),
            Err(err) => (-1, String::new(), err.to_string()),
        };
        let _ = st
            .store
            .append_execution(&SandboxExecution {
                execution_id: Uuid::new_v4(),
                thread_id: thread_uuid,
                command: "prepare_runtime".to_string(),
                exit_code,
                stdout,
                stderr,
                created_at: chrono::Utc::now(),
            })
            .await;
        metrics::counter!("open_harness_orchestrator_sandbox_execution_total").increment(1);
    }

    let facts = st.store.list_facts(thread_uuid).await.unwrap_or_default();
    let tool_records = st.store.list_tool_records(thread_uuid).await.unwrap_or_default();
    let subagent_records = st.store.list_tasks_by_thread(thread_uuid).await.unwrap_or_default();
    let sandbox_records = st.store.list_executions(thread_uuid).await.unwrap_or_default();
    let skills = st.store.list_skills().await.unwrap_or_default();

    let mut base_state = ThreadState::new(thread_id);
    base_state.governance_marks.policy_version = Some(st.governance.policy_version.clone());
    let budget = RunBudget {
        max_turns: st.governance.policies.max_turns.max(1),
        max_subagent_tasks: st.governance.subagents.max_tasks_per_run.max(1),
        max_concurrent_subagents: st.governance.subagents.max_concurrent.max(1),
    };
    let tool_cfg = ToolLoopConfig { assembly: st.governance.tool_assembly() };
    let run_cfg = build_inner_run_config(&ctx, &body, st.governance.as_ref());
    let loop_result = run_agent_loop(
        st.graph.as_ref(),
        st.agent_deps.as_ref(),
        thread_id,
        base_state,
        body.messages.clone(),
        budget,
        &tool_cfg,
        &run_cfg,
    )
    .await;

    match loop_result {
        Ok((final_state, sink)) => Json(json!({
            "ok": true,
            "thread_id": thread_uuid,
            "runtime_engine": "inner",
            "loop_detected": ctx.loop_detected,
            "token_usage_estimate": ctx.token_usage_estimate,
            "todos": ctx.todos,
            "memory_facts": facts,
            "skills": skills,
            "tool_records_count": tool_records.len(),
            "subagent_tasks_count": subagent_records.len(),
            "sandbox_executions_count": sandbox_records.len(),
            "runtime_metadata": inner_runtime_metadata(&final_state, &st.governance.policy_version),
            "agent_events": sink.events,
            "thread_state": final_state,
            "events": []
        })),
        Err(e) => Json(json!({
            "ok": false,
            "error": e.to_string(),
            "thread_id": thread_uuid,
        })),
    }
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
    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!(
        "open_harness_orchestrator_tool_record_total",
        "orchestrator tool records persisted"
    );
    metrics::describe_counter!(
        "open_harness_orchestrator_sandbox_execution_total",
        "orchestrator sandbox executions"
    );
    metrics::describe_counter!(
        "open_harness_orchestrator_subagent_task_total",
        "orchestrator subagent tasks persisted"
    );
    if cfg.storage.mode == "local_fs" {
        let layout = LocalFsLayout::new(&cfg.storage.local_fs_root);
        let _ = layout.ensure_base_dirs();
    }
    let store = Arc::new(LocalFsStateStore::new(&cfg.storage.local_fs_root));
    let governance = Arc::new(
        GovernanceBundle::load_from_dir(&cfg.runtime.governance_root).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "governance load failed; using defaults");
            GovernanceBundle::default()
        }),
    );
    let agent_deps = Arc::new(build_agent_loop_deps(governance.as_ref(), store.clone()));
    let checkpoint_store: Arc<dyn CheckpointStore> = store.clone();
    let checkpoint_port = Arc::new(DynCheckpointStorePort::new(checkpoint_store));
    let graph = Arc::new(GraphRuntime::new(checkpoint_port));
    let app_state = AppState { store, governance, graph, agent_deps };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/openapi.json", get(|| async {
            Json(json!({
                "openapi": "3.1.0",
                "info": {"title": "open-harness-orchestrator", "version": "0.1.0"},
                "paths": {
                    "/healthz": {"get": {"summary": "Health check"}},
                    "/metrics": {"get": {"summary": "Prometheus metrics"}},
                    "/internal/pipeline-check": {"post": {"summary": "Check runtime pipeline"}},
                    "/internal/orchestrate": {"post": {"summary": "Run orchestration and persist state"}}
                }
            }))
        }))
        .route(
            "/metrics",
            get(move || {
                let p = prom.clone();
                async move { p.render() }
            }),
        )
        .route("/internal/pipeline-check", post(pipeline_check))
        .route("/internal/orchestrate", post(run_orchestrate_with_state))
        .layer(TraceLayer::new_for_http());
    let app = app.with_state(app_state);

    let addr: SocketAddr = "0.0.0.0:8083".parse()?;
    tracing::info!(
        %addr,
        engine = %cfg.runtime.engine,
        governance_root = %cfg.runtime.governance_root,
        "open-harness-orchestrator listening"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
