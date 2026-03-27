use app_auth::AuthContext;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use std::time::Duration;
use uuid::Uuid;

use crate::task_access::can_access_task;
use crate::tasks::{bump_task, is_terminal, prune_tasks, MAX_STREAM_CHUNKS};
use crate::{now_ts, AppState};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskRecord {
    pub task_id: String,
    pub thread_id: String,
    pub status: TaskStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: u64,
    pub output_chunks: Vec<String>,
    pub error: Option<String>,
    pub callback_url: Option<String>,
    pub stream: bool,
    pub client_task_id: Option<String>,
    pub tenant_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskDispatchRequest {
    pub input: serde_json::Value,
    #[serde(default)]
    pub configurable: Option<serde_json::Value>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub callback_url: Option<String>,
    #[serde(default)]
    pub client_task_id: Option<String>,
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

pub(crate) async fn dispatch_task(
    State(st): State<AppState>,
    Extension(auth_ctx): Extension<AuthContext>,
    Path(thread_id): Path<String>,
    Json(body): Json<TaskDispatchRequest>,
) -> impl IntoResponse {
    metrics::counter!("open_harness_manage_requests_total").increment(1);
    metrics::counter!("open_harness_manage_task_created_total").increment(1);
    let task_id = body.client_task_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    if let Some(existing) = st.tasks.get(&task_id) {
        if existing.client_task_id.is_some()
            && existing.tenant_id == auth_ctx.tenant_id
            && existing.user_id == auth_ctx.user_id
        {
            return (
                StatusCode::CONFLICT,
                Json(json!({"error":"task_id_conflict","task_id": task_id})),
            );
        }
    }
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
    st.task_workers.spawn(async move {
        run_task_worker(st_clone, spawned_task_id, thread_id, body).await;
    });
    (StatusCode::ACCEPTED, Json(json!({"task_id": task_id, "status": "queued"})))
}

pub(crate) async fn get_task(
    State(st): State<AppState>,
    Extension(auth_ctx): Extension<AuthContext>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    if let Some(task) = st.tasks.get(&task_id) {
        if !can_access_task(&task, &auth_ctx) {
            metrics::counter!("open_harness_manage_task_forbidden_total").increment(1);
            return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"}))).into_response();
        }
        return (StatusCode::OK, Json(json!(task.clone()))).into_response();
    }
    (StatusCode::NOT_FOUND, Json(json!({"error":"task_not_found"}))).into_response()
}

pub(crate) async fn stream_task(
    State(st): State<AppState>,
    Extension(auth_ctx): Extension<AuthContext>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let body_stream = futures::stream::unfold(
        (st, task_id, auth_ctx, 0_u64, false),
        |(st, task_id, auth_ctx, mut seen, sent_end)| async move {
            tokio::time::sleep(Duration::from_millis(800)).await;
            let Some(task) = st.tasks.get(&task_id).map(|v| v.clone()) else {
                let payload =
                    bytes::Bytes::from("event: error\ndata: {\"error\":\"task_not_found\"}\n\n");
                return Some((
                    Ok::<_, std::convert::Infallible>(payload),
                    (st, task_id, auth_ctx, seen, true),
                ));
            };
            if !can_access_task(&task, &auth_ctx) {
                metrics::counter!("open_harness_manage_task_forbidden_total").increment(1);
                let payload =
                    bytes::Bytes::from("event: error\ndata: {\"error\":\"forbidden\"}\n\n");
                return Some((
                    Ok::<_, std::convert::Infallible>(payload),
                    (st, task_id, auth_ctx, seen, true),
                ));
            }
            if task.version > seen {
                seen = task.version;
                let payload = format!("event: task\ndata: {}\n\n", json!(task));
                return Some((
                    Ok(bytes::Bytes::from(payload)),
                    (st, task_id, auth_ctx, seen, false),
                ));
            }
            if is_terminal(&task.status) && !sent_end {
                return Some((
                    Ok(bytes::Bytes::from("event: end\ndata: {\"done\":true}\n\n")),
                    (st, task_id, auth_ctx, seen, true),
                ));
            }
            if sent_end {
                return None;
            }
            Some((Ok(bytes::Bytes::from_static(b"")), (st, task_id, auth_ctx, seen, false)))
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
