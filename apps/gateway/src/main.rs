use app_auth::{
    build_settings, require_auth, shared_state, update_settings, AuthContext, AuthSettings,
    SharedAuthState,
};
use axum::{
    body::Body,
    extract::{Extension, State},
    http::{header, Request, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use config_runtime::{load_cached_or_default, reload_cached, AuthConfig, ModelConfig};
use dashmap::DashMap;
use metrics_exporter_prometheus::PrometheusBuilder;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace::TracerProvider, Resource};
use protocol_compat::{OpenAiChatCompletionsRequest, OpenAiModelItem, OpenAiModelsResponse};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing_subscriber::prelude::*;
use uuid::Uuid;

mod chat_support;

use chat_support::{
    conversation_cache_key, ensure_thread, extract_assistant_text, extract_assistant_texts,
    map_upstream_status, model_api_key,
};

#[derive(Clone)]
struct AppState {
    langgraph_upstream: Arc<RwLock<String>>,
    client: reqwest::Client,
    conversation_map: Arc<DashMap<String, ConversationEntry>>,
    models: Arc<RwLock<Vec<ModelConfig>>>,
    auth_state: SharedAuthState,
}

#[derive(Clone)]
struct ConversationEntry {
    thread_id: String,
    updated_at: Instant,
}

const MAX_CONVERSATIONS: usize = 10_000;
const CONVERSATION_TTL: Duration = Duration::from_secs(60 * 60 * 24);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "gateway_service=info,tower_http=info".into());
    let _otel_provider = init_otel_provider()?;
    if let Some(provider) = _otel_provider.as_ref() {
        let tracer = provider.tracer("open-harness-gateway");
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    let cfg = load_cached_or_default();
    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!("open_harness_gateway_requests_total", "Total proxied requests");
    metrics::describe_counter!(
        "open_harness_gateway_conversation_evicted_total",
        "Conversation cache entries evicted"
    );
    metrics::describe_counter!(
        "open_harness_gateway_stream_error_total",
        "Streaming upstream errors observed"
    );
    let auth_settings = auth_settings_from_config(&cfg.gateway.auth);
    let auth_state =
        shared_state(auth_settings, vec!["/healthz".to_string(), "/metrics".to_string()]);
    let state = AppState {
        langgraph_upstream: Arc::new(RwLock::new(cfg.gateway.langgraph_upstream.clone())),
        client: reqwest::Client::new(),
        conversation_map: Arc::new(DashMap::new()),
        models: Arc::new(RwLock::new(cfg.models.clone())),
        auth_state: auth_state.clone(),
    };

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/openapi.json", get(openapi_spec))
        .route("/v1/models", get(openai_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
        .route("/api/admin/config/reload", post(reload_config))
        .route(
            "/metrics",
            get(move || {
                let p = prom.clone();
                async move { p.render() }
            }),
        )
        .route("/api/langgraph/demo-stream", get(demo_stream))
        .fallback(proxy_langgraph)
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_auth))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.gateway.bind).await?;
    tracing::info!("open-harness-gateway listening on {}", cfg.gateway.bind);
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    metrics::counter!("open_harness_gateway_shutdown_total").increment(1);
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn openapi_spec() -> impl IntoResponse {
    Json(json!({
        "openapi": "3.1.0",
        "info": {"title": "open-harness-gateway", "version": "0.1.0"},
        "components": {
            "schemas": {
                "ChatMessage": {
                    "type": "object",
                    "required": ["role", "content"],
                    "properties": {
                        "role": {"type": "string", "enum": ["system", "user", "assistant", "tool"]},
                        "content": {"oneOf": [{"type": "string"}, {"type": "array"}]},
                        "name": {"type": "string"},
                        "tool_calls": {"type": "array"}
                    }
                },
                "ChatCompletionRequest": {
                    "type": "object",
                    "required": ["model", "messages"],
                    "properties": {
                        "model": {"type": "string", "description": "Model id from /v1/models"},
                        "messages": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/ChatMessage"},
                            "description": "OpenAI-compatible chat messages"
                        },
                        "temperature": {"type": "number"},
                        "max_tokens": {"type": "integer"},
                        "stream": {"type": "boolean"},
                        "user": {"type": "string", "description": "Opaque user id; mapped to thread id"},
                        "tools": {"type": "array"},
                        "tool_choice": {},
                        "response_format": {}
                    }
                },
                "ChatCompletionChoice": {
                    "type": "object",
                    "properties": {
                        "index": {"type": "integer"},
                        "message": {"$ref": "#/components/schemas/ChatMessage"},
                        "finish_reason": {"type": "string"}
                    }
                },
                "ChatCompletionResponse": {
                    "type": "object",
                    "required": ["id", "object", "created", "model", "choices"],
                    "properties": {
                        "id": {"type": "string"},
                        "object": {"type": "string", "enum": ["chat.completion"]},
                        "created": {"type": "integer"},
                        "model": {"type": "string"},
                        "choices": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/ChatCompletionChoice"}
                        },
                        "usage": {"type": "object"}
                    }
                },
                "ChatCompletionChunk": {
                    "type": "object",
                    "description": "SSE chunk when stream=true",
                    "properties": {
                        "id": {"type": "string"},
                        "object": {"type": "string", "enum": ["chat.completion.chunk"]},
                        "choices": {"type": "array"}
                    }
                }
            }
        },
        "paths": {
            "/healthz": {"get": {"summary": "Health check"}},
            "/v1/models": {"get": {"summary": "List models"}},
            "/v1/chat/completions": {"post": {
                "summary": "OpenAI compatible chat completions",
                "requestBody": {
                    "required": true,
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChatCompletionRequest"}}}
                },
                "responses": {
                    "200": {"description": "chat completion", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChatCompletionResponse"}}}}
                }
            }},
            "/api/admin/config/reload": {"post": {"summary": "Reload config"}},
            "/metrics": {"get": {"summary": "Prometheus metrics"}}
        }
    }))
}

fn init_otel_provider() -> anyhow::Result<Option<TracerProvider>> {
    let endpoint = match std::env::var("OPEN_HARNESS_OTLP_ENDPOINT") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return Ok(None),
    };
    let exporter =
        opentelemetry_otlp::SpanExporter::builder().with_tonic().with_endpoint(endpoint).build()?;
    let provider = TracerProvider::builder()
        .with_resource(Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            "open-harness-gateway",
        )]))
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();
    Ok(Some(provider))
}

/// Minimal SSE demo (LangGraph-style `data: {json}\n\n`).
async fn demo_stream() -> impl IntoResponse {
    use protocol_compat::SseEvent;
    let end = SseEvent::End { run_id: "demo-run".into() };
    let payload = serde_json::to_string(&end).unwrap();
    let body = format!("data: {payload}\n\n");
    ([(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")], body)
}

async fn openai_models(State(st): State<AppState>) -> impl IntoResponse {
    let now = Utc::now().timestamp();
    let models = st.models.read().await;
    let data = models
        .iter()
        .map(|m| OpenAiModelItem {
            id: m.name.clone(),
            object: "model".into(),
            created: now,
            owned_by: "open-harness".into(),
        })
        .collect();
    Json(OpenAiModelsResponse { object: "list".into(), data })
}

async fn openai_chat_completions(
    State(st): State<AppState>,
    Extension(auth_ctx): Extension<AuthContext>,
    Json(body): Json<OpenAiChatCompletionsRequest>,
) -> Response {
    let request_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
    let stream = body.stream.unwrap_or(false);
    let models = st.models.read().await.clone();
    if !models.iter().any(|m| m.name == body.model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown model {}", body.model) })),
        )
            .into_response();
    }

    prune_conversation_map(&st.conversation_map);
    let mut configurable = body.configurable.clone().unwrap_or_default();
    let thread_key = conversation_cache_key(&auth_ctx, body.user.as_deref());
    let thread_id = configurable.thread_id.clone().unwrap_or_else(|| {
        st.conversation_map
            .entry(thread_key.clone())
            .or_insert_with(|| ConversationEntry {
                thread_id: Uuid::new_v4().to_string(),
                updated_at: Instant::now(),
            })
            .thread_id
            .clone()
    });
    st.conversation_map.insert(
        thread_key,
        ConversationEntry { thread_id: thread_id.clone(), updated_at: Instant::now() },
    );

    if let Err(e) = ensure_thread(&st, &thread_id).await {
        return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e }))).into_response();
    }

    let mut input = json!({
        "messages": body.messages
    });
    if let Some(tools) = body.tools.clone() {
        input["tools"] = json!(tools);
    }
    if let Some(tool_choice) = body.tool_choice.clone() {
        input["tool_choice"] = tool_choice;
    }
    if let Some(response_format) = body.response_format.clone() {
        input["response_format"] = response_format;
    }
    input["metadata"] = json!({
        "tenant_id": auth_ctx.tenant_id,
        "user_id": auth_ctx.user_id
    });

    configurable.thread_id = Some(thread_id.clone());
    configurable.model_name = Some(body.model.clone());
    configurable.api_key = model_api_key(&models, &body.model);
    configurable.thinking_enabled.get_or_insert(false);
    configurable.is_plan_mode.get_or_insert(false);
    configurable.subagent_enabled.get_or_insert(false);
    configurable.skills_enabled.get_or_insert(true);
    configurable.sandbox_enabled.get_or_insert(false);
    let mut run_req = json!({
        "input": input,
        "config": {
            "configurable": configurable
        }
    });
    if stream {
        let stream_mode = body.stream_mode.clone().unwrap_or_else(|| {
            vec![
                "values".to_string(),
                "messages-tuple".to_string(),
                "end".to_string(),
                "error".to_string(),
            ]
        });
        run_req["stream_mode"] = json!(stream_mode);
    }

    let run_path = if stream { "runs/stream" } else { "runs" };
    let upstream = st.langgraph_upstream.read().await.clone();
    let url = format!("{}/threads/{thread_id}/{run_path}", upstream.trim_end_matches('/'));
    let upstream = match st.client.post(&url).json(&run_req).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e.to_string() })))
                .into_response();
        }
    };

    if !upstream.status().is_success() {
        let upstream_status = upstream.status();
        let mapped_status = map_upstream_status(upstream_status);
        let body = upstream.text().await.unwrap_or_else(|_| "upstream error".into());
        return (mapped_status, Json(json!({ "status": upstream_status.as_u16(), "error": body })))
            .into_response();
    }

    if stream {
        let stream = upstream.bytes_stream();
        let init_chunk = format!(
            "data: {}\n\n",
            json!({
                "id": request_id.clone(),
                "object": "chat.completion.chunk",
                "choices": [{ "index": 0, "delta": { "role": "assistant" }, "finish_reason": serde_json::Value::Null }]
            })
        );
        let body_stream = futures::stream::unfold(
            (
                stream,
                request_id.clone(),
                false,
                vec![Ok::<_, std::convert::Infallible>(bytes::Bytes::from(init_chunk))],
            ),
            |(mut s, req_id, done_sent, mut pending)| async move {
                if let Some(item) = pending.pop() {
                    return Some((item, (s, req_id, done_sent, pending)));
                }
                match futures::StreamExt::next(&mut s).await {
                    Some(Ok(chunk)) => {
                        let payload = String::from_utf8_lossy(&chunk);
                        let mut out = Vec::new();
                        for text in extract_assistant_texts(&payload) {
                            out.push(Ok::<_, std::convert::Infallible>(bytes::Bytes::from(format!(
                                "data: {}\n\n",
                                json!({
                                    "id": req_id,
                                    "object": "chat.completion.chunk",
                                    "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": serde_json::Value::Null }]
                                })
                            ))));
                        }
                        if out.is_empty() {
                            return Some((
                                Ok(bytes::Bytes::from_static(b"")),
                                (s, req_id, done_sent, out),
                            ));
                        }
                        out.reverse();
                        let next = out.pop().expect("non-empty");
                        Some((next, (s, req_id, done_sent, out)))
                    }
                    Some(Err(err)) => {
                        if done_sent {
                            None
                        } else {
                            metrics::counter!("open_harness_gateway_stream_error_total")
                                .increment(1);
                            let error_chunk = format!(
                                "data: {}\n\n",
                                json!({
                                    "id": req_id,
                                    "object":"chat.completion.chunk",
                                    "choices":[{"index":0,"delta":{"content": format!("upstream_stream_error: {err}")},"finish_reason":"error"}]
                                })
                            );
                            Some((
                                Ok(bytes::Bytes::from(format!("{error_chunk}data: [DONE]\n\n"))),
                                (s, req_id, true, vec![]),
                            ))
                        }
                    }
                    None => {
                        if done_sent {
                            None
                        } else {
                            Some((
                                Ok(bytes::Bytes::from(
                                    "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
                                )),
                                (s, req_id, true, vec![]),
                            ))
                        }
                    }
                }
            },
        );
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build").into_response());
    }

    let text: String = (upstream.text().await).unwrap_or_default();
    let content = extract_assistant_text(&text).unwrap_or_else(|| "ok".to_string());
    Json(json!({
        "id": request_id,
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": body.model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }]
    }))
    .into_response()
}

fn prune_conversation_map(conversation_map: &DashMap<String, ConversationEntry>) {
    let now = Instant::now();
    let before_ttl = conversation_map.len();
    conversation_map.retain(|_, entry| now.duration_since(entry.updated_at) <= CONVERSATION_TTL);
    let ttl_evicted = before_ttl.saturating_sub(conversation_map.len());
    if ttl_evicted > 0 {
        metrics::counter!("open_harness_gateway_conversation_evicted_total")
            .increment(ttl_evicted as u64);
    }
    if conversation_map.len() <= MAX_CONVERSATIONS {
        return;
    }
    let mut candidates: Vec<(String, Instant)> =
        conversation_map.iter().map(|entry| (entry.key().clone(), entry.updated_at)).collect();
    candidates.sort_by_key(|(_, updated_at)| *updated_at);
    let remove_n = candidates.len().saturating_sub(MAX_CONVERSATIONS);
    for (key, _) in candidates.into_iter().take(remove_n) {
        conversation_map.remove(&key);
    }
    if remove_n > 0 {
        metrics::counter!("open_harness_gateway_conversation_evicted_total")
            .increment(remove_n as u64);
    }
}

fn auth_settings_from_config(cfg: &AuthConfig) -> AuthSettings {
    build_settings(cfg.enabled, cfg.api_keys.clone(), cfg.bearer_tokens.clone())
}

async fn proxy_langgraph(State(st): State<AppState>, req: Request<Body>) -> impl IntoResponse {
    metrics::counter!("open_harness_gateway_requests_total").increment(1);
    let upstream = st.langgraph_upstream.read().await.clone();
    let path_and_query = req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let prefix = "/api/langgraph";
    let rest = path_and_query.strip_prefix(prefix).unwrap_or(path_and_query);
    let target = format!("{}{}", upstream.trim_end_matches('/'), rest);

    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid body").into_response(),
    };

    let mut rb = st.client.request(method, &target);
    for (k, v) in headers.iter() {
        if k == axum::http::header::HOST || k == axum::http::header::CONNECTION {
            continue;
        }
        rb = rb.header(k, v);
    }
    let resp = match rb.body(body_bytes).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("proxy error: {e}");
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e.to_string() })))
                .into_response();
        }
    };

    let status = resp.status();
    let mut res = http::Response::builder().status(status);
    for (k, v) in resp.headers().iter() {
        if k == reqwest::header::TRANSFER_ENCODING {
            continue;
        }
        res = res.header(k, v);
    }
    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream body").into_response(),
    };
    res.body(Body::from(bytes))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build").into_response())
}

async fn reload_config(State(st): State<AppState>) -> impl IntoResponse {
    match reload_cached() {
        Ok(cfg) => {
            *st.models.write().await = cfg.models;
            *st.langgraph_upstream.write().await = cfg.gateway.langgraph_upstream;
            update_settings(&st.auth_state, auth_settings_from_config(&cfg.gateway.auth)).await;
            (StatusCode::OK, Json(json!({"reloaded": true}))).into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"reloaded": false, "error": err.to_string()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_conversation_map_removes_expired_entries() {
        let map = DashMap::new();
        map.insert(
            "expired".to_string(),
            ConversationEntry {
                thread_id: "t1".to_string(),
                updated_at: Instant::now() - CONVERSATION_TTL - Duration::from_secs(1),
            },
        );
        map.insert(
            "fresh".to_string(),
            ConversationEntry { thread_id: "t2".to_string(), updated_at: Instant::now() },
        );

        prune_conversation_map(&map);

        assert!(!map.contains_key("expired"));
        assert!(map.contains_key("fresh"));
    }

    #[test]
    fn prune_conversation_map_enforces_max_capacity() {
        let map = DashMap::new();
        for i in 0..(MAX_CONVERSATIONS + 8) {
            map.insert(
                format!("k{i}"),
                ConversationEntry {
                    thread_id: format!("t{i}"),
                    updated_at: Instant::now() - Duration::from_secs(i as u64),
                },
            );
        }

        prune_conversation_map(&map);

        assert_eq!(map.len(), MAX_CONVERSATIONS);
    }
}
