use app_auth::AuthContext;
use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{TimeZone, Utc};
use dashmap::mapref::entry::Entry;
use hmac::{Hmac, Mac};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use state_abstraction::ManageTaskRecord;
use std::time::Duration;
use uuid::Uuid;

use crate::security::sanitize_thread_id;
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

const WEBHOOK_RETRY_MAX_ATTEMPTS: usize = 3;
const WEBHOOK_RETRY_BASE_MS: u64 = 250;

fn task_status_name(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
    }
}

fn task_to_store_record(task: &TaskRecord) -> Option<ManageTaskRecord> {
    let created_at = Utc.timestamp_opt(task.created_at, 0).single()?;
    let updated_at = Utc.timestamp_opt(task.updated_at, 0).single()?;
    let version = i64::try_from(task.version).ok()?;
    Some(ManageTaskRecord {
        task_id: task.task_id.clone(),
        thread_id: task.thread_id.clone(),
        status: task_status_name(&task.status).to_string(),
        output_chunks: task.output_chunks.clone(),
        error: task.error.clone(),
        callback_url: task.callback_url.clone(),
        stream: task.stream,
        client_task_id: task.client_task_id.clone(),
        tenant_id: task.tenant_id.clone(),
        user_id: task.user_id.clone(),
        created_at,
        updated_at,
        version,
    })
}

async fn persist_task_record(st: &AppState, task: &TaskRecord) {
    let Some(record) = task_to_store_record(task) else {
        tracing::warn!(task_id = %task.task_id, "skip persistence due to invalid task timestamp/version");
        return;
    };
    if let Err(err) = st.manage_tasks.upsert_task(&record).await {
        tracing::warn!(task_id = %task.task_id, error = %err, "persist manage task failed");
    }
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
    let mut delay = Duration::from_millis(WEBHOOK_RETRY_BASE_MS);
    for attempt in 1..=WEBHOOK_RETRY_MAX_ATTEMPTS {
        let timestamp = now_ts();
        let nonce = Uuid::new_v4().to_string();
        let mut req = st.http_client.post(&callback_url).header("content-type", "application/json");
        if let Some(secret) = st.webhook_secret.as_ref() {
            if let Some(signature) = sign_payload(secret, &body, timestamp, &nonce) {
                req = req
                    .header("x-open-harness-signature", signature)
                    .header("x-open-harness-timestamp", timestamp.to_string())
                    .header("x-open-harness-nonce", nonce);
            }
        }
        let req = req.body(body.clone());
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                metrics::counter!("open_harness_manage_webhook_success_total").increment(1);
                return;
            }
            Ok(resp) => {
                tracing::warn!(
                    task_id = %task.task_id,
                    attempt,
                    status = %resp.status(),
                    "webhook call failed"
                );
            }
            Err(err) => {
                tracing::warn!(task_id = %task.task_id, attempt, error = %err, "webhook call error");
            }
        }
        if attempt < WEBHOOK_RETRY_MAX_ATTEMPTS {
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2);
        }
    }
    metrics::counter!("open_harness_manage_webhook_failure_total").increment(1);
}

async fn update_task_status(
    st: &AppState,
    task_id: &str,
    status: TaskStatus,
    output: Option<String>,
    error: Option<String>,
    metric: Option<&'static str>,
) -> Option<TaskRecord> {
    let task = bump_task(st, task_id, status, output, error)?;
    if let Some(name) = metric {
        metrics::counter!(name).increment(1);
    }
    persist_task_record(st, &task).await;
    post_webhook(st, &task).await;
    Some(task)
}

fn conflict_response(task_id: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::CONFLICT, Json(json!({"error":"task_id_conflict","task_id": task_id})))
}

fn parse_task_status(status: &str) -> Option<TaskStatus> {
    match status {
        "queued" => Some(TaskStatus::Queued),
        "running" => Some(TaskStatus::Running),
        "completed" => Some(TaskStatus::Completed),
        "failed" => Some(TaskStatus::Failed),
        _ => None,
    }
}

fn store_record_to_task(record: ManageTaskRecord) -> Option<TaskRecord> {
    let status = parse_task_status(&record.status)?;
    let created_at = record.created_at.timestamp();
    let updated_at = record.updated_at.timestamp();
    let version = u64::try_from(record.version).ok()?;
    Some(TaskRecord {
        task_id: record.task_id,
        thread_id: record.thread_id,
        status,
        created_at,
        updated_at,
        version,
        output_chunks: record.output_chunks,
        error: record.error,
        callback_url: record.callback_url,
        stream: record.stream,
        client_task_id: record.client_task_id,
        tenant_id: record.tenant_id,
        user_id: record.user_id,
    })
}

async fn fetch_task_record(st: &AppState, task_id: &str) -> Option<TaskRecord> {
    if let Some(task) = st.tasks.get(task_id) {
        return Some(task.clone());
    }
    match st.manage_tasks.get_task(task_id).await {
        Ok(Some(record)) => {
            let Some(converted) = store_record_to_task(record) else {
                tracing::warn!(task_id, "invalid persisted task record");
                return None;
            };
            // Read-through cache: avoids repeated persistent lookups in stream polling.
            let cached = converted.clone();
            st.tasks.insert(task_id.to_string(), cached);
            Some(converted)
        }
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(task_id, error = %err, "load persisted task failed");
            None
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
    let Some(thread_id) = sanitize_thread_id(&thread_id) else {
        metrics::counter!("open_harness_manage_invalid_path_total").increment(1);
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_thread_id"})));
    };
    metrics::counter!("open_harness_manage_task_created_total").increment(1);
    let task_id = body.client_task_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = now_ts();
    let new_task = TaskRecord {
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

    let task = match st.tasks.entry(task_id.clone()) {
        Entry::Occupied(existing) => {
            if can_access_task(existing.get(), &auth_ctx) {
                return conflict_response(&task_id);
            }
            metrics::counter!("open_harness_manage_task_forbidden_total").increment(1);
            return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"})));
        }
        Entry::Vacant(slot) => {
            let inserted = slot.insert(new_task);
            inserted.clone()
        }
    };
    persist_task_record(&st, &task).await;
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
    if let Some(task) = fetch_task_record(&st, &task_id).await {
        if !can_access_task(&task, &auth_ctx) {
            metrics::counter!("open_harness_manage_task_forbidden_total").increment(1);
            return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"}))).into_response();
        }
        return (StatusCode::OK, Json(json!(task))).into_response();
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
            let Some(task) = fetch_task_record(&st, &task_id).await else {
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
    if update_task_status(&st, &task_id, TaskStatus::Running, None, None, None).await.is_none() {
        tracing::warn!(task_id = %task_id, "task missing before worker start");
        return;
    }
    let (run_url, run_req) = build_run_request(&st, &thread_id, &body).await;
    if body.stream {
        run_streaming_task(&st, &task_id, run_url, run_req).await;
        return;
    }
    run_single_response_task(&st, &task_id, run_url, run_req).await;
}

async fn build_run_request(
    st: &AppState,
    thread_id: &str,
    body: &TaskDispatchRequest,
) -> (String, serde_json::Value) {
    let langgraph_url = st.langgraph_url.read().await.clone();
    let run_url = format!(
        "{}/threads/{thread_id}/runs{}",
        langgraph_url.trim_end_matches('/'),
        if body.stream { "/stream" } else { "" }
    );
    let run_req = json!({
        "input": body.input,
        "config": {
            "configurable": body.configurable.clone().unwrap_or_else(|| json!({}))
        },
        "stream_mode": ["values", "messages-tuple", "end", "error"]
    });
    (run_url, run_req)
}

async fn run_streaming_task(
    st: &AppState,
    task_id: &str,
    run_url: String,
    run_req: serde_json::Value,
) {
    match st.http_client.post(run_url).json(&run_req).send().await {
        Ok(resp) if resp.status().is_success() => {
            let mut stream = resp.bytes_stream();
            let mut total = 0_usize;
            while let Some(next) = futures::StreamExt::next(&mut stream).await {
                match next {
                    Ok(chunk) => {
                        if total >= MAX_STREAM_CHUNKS {
                            let _ = update_task_status(
                                st,
                                task_id,
                                TaskStatus::Failed,
                                None,
                                Some(format!(
                                    "stream output exceeded chunk limit {}",
                                    MAX_STREAM_CHUNKS
                                )),
                                Some("open_harness_manage_task_failed_total"),
                            )
                            .await;
                            return;
                        }
                        let payload = String::from_utf8_lossy(&chunk).to_string();
                        if let Some(updated) =
                            bump_task(st, task_id, TaskStatus::Running, Some(payload), None)
                        {
                            persist_task_record(st, &updated).await;
                        }
                        total += 1;
                    }
                    Err(err) => {
                        let _ = update_task_status(
                            st,
                            task_id,
                            TaskStatus::Failed,
                            None,
                            Some(format!("stream read failed: {err}")),
                            Some("open_harness_manage_task_failed_total"),
                        )
                        .await;
                        return;
                    }
                }
            }
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Completed,
                None,
                None,
                Some("open_harness_manage_task_completed_total"),
            )
            .await;
        }
        Ok(resp) => {
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream status {}", resp.status())),
                Some("open_harness_manage_task_failed_total"),
            )
            .await;
        }
        Err(err) => {
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream error {err}")),
                Some("open_harness_manage_task_failed_total"),
            )
            .await;
        }
    }
}

async fn run_single_response_task(
    st: &AppState,
    task_id: &str,
    run_url: String,
    run_req: serde_json::Value,
) {
    match st.http_client.post(run_url).json(&run_req).send().await {
        Ok(resp) if resp.status().is_success() => {
            let output = resp.text().await.unwrap_or_default();
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Completed,
                Some(output),
                None,
                Some("open_harness_manage_task_completed_total"),
            )
            .await;
        }
        Ok(resp) => {
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream status {}", resp.status())),
                Some("open_harness_manage_task_failed_total"),
            )
            .await;
        }
        Err(err) => {
            let _ = update_task_status(
                st,
                task_id,
                TaskStatus::Failed,
                None,
                Some(format!("upstream error {err}")),
                Some("open_harness_manage_task_failed_total"),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use state_abstraction::ManageTaskRecord;

    #[test]
    fn task_status_name_maps_as_expected() {
        assert_eq!(task_status_name(&TaskStatus::Queued), "queued");
        assert_eq!(task_status_name(&TaskStatus::Running), "running");
        assert_eq!(task_status_name(&TaskStatus::Completed), "completed");
        assert_eq!(task_status_name(&TaskStatus::Failed), "failed");
    }

    #[test]
    fn parse_task_status_handles_known_and_unknown_values() {
        assert!(matches!(parse_task_status("queued"), Some(TaskStatus::Queued)));
        assert!(matches!(parse_task_status("running"), Some(TaskStatus::Running)));
        assert!(matches!(parse_task_status("completed"), Some(TaskStatus::Completed)));
        assert!(matches!(parse_task_status("failed"), Some(TaskStatus::Failed)));
        assert!(parse_task_status("unknown").is_none());
    }

    #[test]
    fn store_record_to_task_rejects_invalid_status() {
        let record = ManageTaskRecord {
            task_id: "task-1".to_string(),
            thread_id: "thread-1".to_string(),
            status: "bogus".to_string(),
            output_chunks: vec![],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "tenant-a".to_string(),
            user_id: "user-a".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        };
        assert!(store_record_to_task(record).is_none());
    }

    #[test]
    fn store_record_to_task_maps_valid_record() {
        let now = Utc::now();
        let record = ManageTaskRecord {
            task_id: "task-1".to_string(),
            thread_id: "thread-1".to_string(),
            status: "running".to_string(),
            output_chunks: vec!["chunk-1".to_string()],
            error: None,
            callback_url: Some("https://example.com/cb".to_string()),
            stream: true,
            client_task_id: Some("client-1".to_string()),
            tenant_id: "tenant-a".to_string(),
            user_id: "user-a".to_string(),
            created_at: now,
            updated_at: now,
            version: 2,
        };
        let task = store_record_to_task(record).expect("valid record must map");
        assert!(matches!(task.status, TaskStatus::Running));
        assert_eq!(task.task_id, "task-1");
        assert_eq!(task.thread_id, "thread-1");
        assert_eq!(task.output_chunks, vec!["chunk-1".to_string()]);
        assert!(task.stream);
        assert_eq!(task.client_task_id, Some("client-1".to_string()));
    }

    #[test]
    fn store_record_to_task_rejects_negative_version() {
        let record = ManageTaskRecord {
            task_id: "task-1".to_string(),
            thread_id: "thread-1".to_string(),
            status: "queued".to_string(),
            output_chunks: vec![],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "tenant-a".to_string(),
            user_id: "user-a".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: -1,
        };
        assert!(store_record_to_task(record).is_none());
    }
}
