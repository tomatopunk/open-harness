use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use config_runtime::{load_or_default, resolve_env_var_ref, ModelConfig};
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
    Json(OpenAiModelsResponse {
        object: "list".into(),
        data,
    })
}

async fn openai_chat_completions(
    State(st): State<AppState>,
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

    let thread_key = body.user.unwrap_or_else(|| "default".to_string());
    let thread_id = st
        .conversation_map
        .entry(thread_key)
        .or_insert_with(|| Uuid::new_v4().to_string())
        .clone();

    if let Err(e) = ensure_thread(&st, &thread_id).await {
        return (StatusCode::BAD_GATEWAY, Json(json!({ "error": e }))).into_response();
    }

    let user_content = body
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let run_req = json!({
        "input": {
            "messages": [{ "role": "user", "content": user_content }]
        },
        "config": {
            "configurable": {
                "model_name": body.model,
                "api_key": model_api_key(&st.models, &body.model)
            }
        },
        "stream_mode": ["values", "messages-tuple", "end"]
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
        return (StatusCode::BAD_GATEWAY, Json(json!({ "status": status.as_u16(), "error": body })))
            .into_response();
    }

    if stream {
        let stream = upstream.bytes_stream();
        let body_stream =
            futures::stream::unfold((stream, request_id.clone()), |(mut s, req_id)| async move {
            match futures::StreamExt::next(&mut s).await {
                Some(Ok(chunk)) => {
                    let payload = String::from_utf8_lossy(&chunk);
                    let content = extract_assistant_text(&payload);
                    let sse_chunk = if let Some(text) = content {
                        let chunk_id = req_id.clone();
                        format!(
                            "data: {}\n\n",
                            json!({
                                "id": chunk_id,
                                "object": "chat.completion.chunk",
                                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": serde_json::Value::Null }]
                            })
                        )
                    } else {
                        "data: [DONE]\n\n".to_string()
                    };
                    Some((
                        Ok::<_, std::convert::Infallible>(bytes::Bytes::from(sse_chunk)),
                        (s, req_id),
                    ))
                }
                Some(Err(_)) => None,
                None => None,
            }
        });
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build").into_response());
    }

    let text = match upstream.text().await {
        Ok(t) => t,
        Err(_) => String::new(),
    };
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

fn model_api_key(models: &[ModelConfig], model_name: &str) -> Option<String> {
    models
        .iter()
        .find(|m| m.name == model_name)
        .and_then(|m| m.api_key.as_ref())
        .map(|v| resolve_env_var_ref(v))
}

fn extract_assistant_text(payload: &str) -> Option<String> {
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
            if let Some(content) = v
                .get("data")
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                return Some(content.to_string());
            }
            if let Some(content) = v
                .get("content")
                .and_then(|c| c.as_str())
            {
                return Some(content.to_string());
            }
        }
    }
    None
}

async fn ensure_thread(st: &AppState, thread_id: &str) -> Result<(), String> {
    let url = format!("{}/threads", st.langgraph_upstream.trim_end_matches('/'));
    let body = ThreadCreate {
        thread_id: Some(thread_id.to_string()),
        metadata: None,
    };
    let resp = st
        .client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
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
