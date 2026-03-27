use app_auth::AuthContext;

use crate::TaskRecord;

pub fn can_access_task(task: &TaskRecord, auth_ctx: &AuthContext) -> bool {
    task.tenant_id == auth_ctx.tenant_id && task.user_id == auth_ctx.user_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStatus;

    fn sample_task() -> TaskRecord {
        TaskRecord {
            task_id: "task-1".to_string(),
            thread_id: "thread-1".to_string(),
            status: TaskStatus::Queued,
            created_at: 1,
            updated_at: 1,
            version: 1,
            output_chunks: vec![],
            error: None,
            callback_url: None,
            stream: false,
            client_task_id: None,
            tenant_id: "tenant-a".to_string(),
            user_id: "user-a".to_string(),
        }
    }

    #[test]
    fn allows_owner_access() {
        let auth = AuthContext { tenant_id: "tenant-a".to_string(), user_id: "user-a".to_string() };
        assert!(can_access_task(&sample_task(), &auth));
    }

    #[test]
    fn blocks_cross_tenant_access() {
        let auth = AuthContext { tenant_id: "tenant-b".to_string(), user_id: "user-a".to_string() };
        assert!(!can_access_task(&sample_task(), &auth));
    }
}
