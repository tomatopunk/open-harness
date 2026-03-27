use app_auth::AuthContext;

use crate::TaskRecord;

pub fn can_access_task(task: &TaskRecord, auth_ctx: &AuthContext) -> bool {
    task.tenant_id == auth_ctx.tenant_id && task.user_id == auth_ctx.user_id
}
