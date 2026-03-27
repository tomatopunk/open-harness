use axum::{
    body::Body,
    extract::{Extension, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use config_runtime::{load_or_default, resolve_env_var_ref, AuthConfig, ModelConfig};
use dashmap::DashMap;
use metrics_exporter_prometheus::PrometheusBuilder;
use protocol_compat::{
    OpenAiChatCompletionsRequest, OpenAiModelItem, OpenAiModelsResponse, ThreadCreate,
};
use serde_json::json;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    langgraph_upstream: String,
    client: reqwest::Client,
    conversation_map: Arc<DashMap<String, String>>,
    models: Vec<ModelConfig>,
}

#[derive(Clone)]
struct AuthSettings {
    enabled: bool,
    api_keys: Arc<Vec<String>>,
    bearer_tokens: Arc<Vec<String>>,
}

#[derive(Clone, Debug)]
struct AuthContext {
    tenant_id: String,
    user_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gateway_service=info,tower_http=info".into()),
        )
        .init();

    let cfg = load_or_default();
    let prom = PrometheusBuilder::new().install_recorder().expect("prometheus recorder");
    metrics::describe_counter!("open_harness_gateway_requests_total", "Total proxied requests");
    let state = AppState {
        langgraph_upstream: cfg.gateway.langgraph_upstream.clone(),
        client: reqwest::Client::new(),
        conversation_map: Arc::new(DashMap::new()),
        models: cfg.models.clone(),
    };
    let auth_settings = auth_settings_from_config(&cfg.gateway.auth);

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/models", get(openai_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
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
        .layer(middleware::from_fn_with_state(auth_settings, require_auth))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.gateway.bind).await?;
    tracing::info!("open-harness-gateway listening on {}", cfg.gateway.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
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
    let data = st
        .models
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
    if !st.models.iter().any(|m| m.name == body.model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown model {}", body.model) })),
        )
            .into_response();
    }

    let mut configurable = body.configurable.clone().unwrap_or_default();
    let thread_key = body.user.clone().unwrap_or_else(|| "default".to_string());
    let thread_id = configurable.thread_id.clone().unwrap_or_else(|| {
        st.conversation_map
            .entry(thread_key.clone())
            .or_insert_with(|| Uuid::new_v4().to_string())
            .clone()
    });
    st.conversation_map.insert(thread_key, thread_id.clone());

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
    configurable.api_key = model_api_key(&st.models, &body.model);
    configurable.thinking_enabled.get_or_insert(false);
    configurable.is_plan_mode.get_or_insert(false);
    configurable.subagent_enabled.get_or_insert(false);
    configurable.skills_enabled.get_or_insert(true);
    configurable.sandbox_enabled.get_or_insert(false);
    let stream_mode = body.stream_mode.clone().unwrap_or_else(|| {
        vec![
            "values".to_string(),
            "messages-tuple".to_string(),
            "end".to_string(),
            "error".to_string(),
        ]
    });

    let run_req = json!({
        "input": input,
        "config": {
            "configurable": configurable
        },
        "stream_mode": stream_mode
    });

    let url = format!(
        "{}/threads/{}/runs/stream",
        st.langgraph_upstream.trim_end_matches('/'),
        thread_id
    );
    let upstream = match st.client.post(&url).json(&run_req).send().await {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e.to_string() })))
                .into_response();
        }
    };

    if !upstream.status().is_success() {
        let status = upstream.status();
        let body = upstream.text().await.unwrap_or_else(|_| "upstream error".into());
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "status": status.as_u16(), "error": body })),
        )
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
                    Some(Err(_)) => {
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

fn auth_settings_from_config(cfg: &AuthConfig) -> AuthSettings {
    AuthSettings {
        enabled: cfg.enabled,
        api_keys: Arc::new(cfg.api_keys.clone()),
        bearer_tokens: Arc::new(cfg.bearer_tokens.clone()),
    }
}

fn is_public_path(path: &str) -> bool {
    path == "/healthz" || path == "/metrics"
}

fn parse_bearer(value: &str) -> Option<&str> {
    value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))
}

fn extract_auth_context(req: &Request<Body>) -> AuthContext {
    let tenant_id = req
        .headers()
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string();
    let user_id = req
        .headers()
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();
    AuthContext { tenant_id, user_id }
}

fn is_authorized(req: &Request<Body>, settings: &AuthSettings) -> bool {
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        if settings.api_keys.iter().any(|k| k == v) {
            return true;
        }
    }
    if let Some(v) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = parse_bearer(v) {
            if settings.bearer_tokens.iter().any(|k| k == token) {
                return true;
            }
        }
    }
    false
}

async fn require_auth(
    State(settings): State<AuthSettings>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if is_public_path(req.uri().path()) {
        return next.run(req).await;
    }
    if settings.enabled && !is_authorized(&req, &settings) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unauthorized" })))
            .into_response();
    }
    let auth_ctx = extract_auth_context(&req);
    req.extensions_mut().insert(auth_ctx);
    next.run(req).await
}

fn model_api_key(models: &[ModelConfig], model_name: &str) -> Option<String> {
    models
        .iter()
        .find(|m| m.name == model_name)
        .and_then(|m| m.api_key.as_ref())
        .map(|v| resolve_env_var_ref(v))
}

fn extract_assistant_text(payload: &str) -> Option<String> {
    extract_assistant_texts(payload).into_iter().next()
}

fn extract_assistant_texts(payload: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in payload.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let data = trimmed.trim_start_matches("data:").trim();
        if data == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(content) =
                v.get("data").and_then(|d| d.get("content")).and_then(|c| c.as_str())
            {
                out.push(content.to_string());
                continue;
            }
            if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                out.push(content.to_string());
            }
        }
    }
    out
}

async fn ensure_thread(st: &AppState, thread_id: &str) -> Result<(), String> {
    let url = format!("{}/threads", st.langgraph_upstream.trim_end_matches('/'));
    let body = ThreadCreate { thread_id: Some(thread_id.to_string()), metadata: None };
    let resp = st.client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() || resp.status().as_u16() == 409 {
        Ok(())
    } else {
        Err(format!("thread create failed {}", resp.status()))
    }
}

async fn proxy_langgraph(State(st): State<AppState>, req: Request<Body>) -> impl IntoResponse {
    metrics::counter!("open_harness_gateway_requests_total").increment(1);
    let path_and_query = req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let prefix = "/api/langgraph";
    let rest = path_and_query.strip_prefix(prefix).unwrap_or(path_and_query);
    let target = format!("{}{}", st.langgraph_upstream.trim_end_matches('/'), rest);

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
