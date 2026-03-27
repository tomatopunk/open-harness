use crate::{now_ts, AppState, TaskRecord, TaskStatus};

pub const MAX_STREAM_CHUNKS: usize = 256;

pub fn is_terminal(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Completed | TaskStatus::Failed)
}

pub fn prune_tasks(tasks: &dashmap::DashMap<String, TaskRecord>, capacity: usize) {
    if tasks.len() <= capacity {
        return;
    }
    let mut entries: Vec<(String, i64)> = tasks
        .iter()
        .filter(|v| is_terminal(&v.value().status))
        .map(|v| (v.key().clone(), v.value().updated_at))
        .collect();
    if entries.is_empty() {
        return;
    }
    entries.sort_by_key(|(_, ts)| *ts);
    let remove_n = tasks.len().saturating_sub(capacity).min(entries.len());
    for (task_id, _) in entries.into_iter().take(remove_n) {
        tasks.remove(&task_id);
    }
}

pub fn bump_task(
    st: &AppState,
    task_id: &str,
    status: TaskStatus,
    output: Option<String>,
    error: Option<String>,
) -> Option<TaskRecord> {
    let mut updated = st.tasks.get_mut(task_id)?;
    updated.status = status;
    updated.updated_at = now_ts();
    updated.version += 1;
    if let Some(chunk) = output {
        updated.output_chunks.push(chunk);
        if updated.output_chunks.len() > 200 {
            let drain_n = updated.output_chunks.len().saturating_sub(200);
            updated.output_chunks.drain(0..drain_n);
        }
    }
    if error.is_some() {
        updated.error = error;
    }
    Some(updated.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(task_id: &str, status: TaskStatus, updated_at: i64) -> TaskRecord {
        TaskRecord {
            task_id: task_id.to_string(),
            thread_id: "t".to_string(),
            status,
            created_at: updated_at,
            updated_at,
            version: 1,
            output_chunks: vec![],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "tenant".to_string(),
            user_id: "user".to_string(),
        }
    }

    #[test]
    fn prune_tasks_only_removes_terminal_items() {
        let tasks = dashmap::DashMap::new();
        tasks.insert("running".to_string(), make_task("running", TaskStatus::Running, 1));
        tasks.insert("queued".to_string(), make_task("queued", TaskStatus::Queued, 2));
        tasks.insert("done_old".to_string(), make_task("done_old", TaskStatus::Completed, 3));
        tasks.insert("failed_new".to_string(), make_task("failed_new", TaskStatus::Failed, 4));

        prune_tasks(&tasks, 1);

        assert!(tasks.contains_key("running"));
        assert!(tasks.contains_key("queued"));
        assert!(!tasks.contains_key("done_old"));
        assert!(!tasks.contains_key("failed_new"));
    }

    #[test]
    fn prune_tasks_respects_total_capacity() {
        let tasks = dashmap::DashMap::new();
        tasks.insert("running1".to_string(), make_task("running1", TaskStatus::Running, 1));
        tasks.insert("running2".to_string(), make_task("running2", TaskStatus::Running, 2));
        tasks.insert("done1".to_string(), make_task("done1", TaskStatus::Completed, 3));
        tasks.insert("done2".to_string(), make_task("done2", TaskStatus::Failed, 4));
        tasks.insert("done3".to_string(), make_task("done3", TaskStatus::Completed, 5));

        prune_tasks(&tasks, 3);

        assert_eq!(tasks.len(), 3);
        assert!(tasks.contains_key("running1"));
        assert!(tasks.contains_key("running2"));
        assert!(tasks.contains_key("done3"));
    }
}
