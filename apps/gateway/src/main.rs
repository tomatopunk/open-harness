use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use config_runtime::load_or_default;
use metrics_exporter_prometheus::PrometheusBuilder;
use serde_json::json;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    langgraph_upstream: String,
    client: reqwest::Client,
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
    };

    let app = Router::new()
        .route("/healthz", get(health))
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
