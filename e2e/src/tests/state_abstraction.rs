use serde_json::json;
use state_abstraction::{
    CreateSessionRequest, ForkSessionRequest, SessionContext, SessionCore, SessionLifecycleState,
    SessionPolicy,
};
use tokio;
use uuid::Uuid;

#[tokio::test]
async fn test_session_core_create_attach_fork_and_close_flow() {
    let session_core = SessionCore::new();
    let thread_id = Uuid::new_v4();

    let mut context = SessionContext::new();
    context.insert("channel", json!("manage"));
    context.insert("user", json!("murphy"));

    let mut parent_policy = SessionPolicy::new();
    parent_policy.insert("mode", json!("safe"));
    parent_policy.insert("max_iterations", json!(12));

    let session = session_core
        .create_session(CreateSessionRequest {
            attached_thread_id: Some(thread_id),
            context: context.clone(),
            policy: parent_policy.clone(),
        })
        .await
        .unwrap();

    assert!(session.parent_session_id.is_none());
    assert_eq!(session.attached_thread_id, Some(thread_id));
    assert_eq!(session.context, context);
    assert_eq!(session.policy, parent_policy);

    let attached = session_core.attach_session(session.session_id).await.unwrap();
    assert_eq!(attached.session_id, session.session_id);

    let mut local_policy = SessionPolicy::new();
    local_policy.insert("max_iterations", json!(3));
    local_policy.insert("sandbox", json!("restricted"));

    let child = session_core
        .fork_session(ForkSessionRequest {
            parent_session_id: session.session_id,
            attached_thread_id: None,
            local_policy: local_policy.clone(),
        })
        .await
        .unwrap();

    assert_eq!(child.parent_session_id, Some(session.session_id));
    assert_eq!(child.attached_thread_id, Some(thread_id));
    assert_eq!(child.context, context);
    assert_eq!(child.local_policy, local_policy);
    assert_eq!(child.policy.get("mode"), Some(&json!("safe")));
    assert_eq!(child.policy.get("max_iterations"), Some(&json!(3)));
    assert_eq!(child.policy.get("sandbox"), Some(&json!("restricted")));

    let reloaded_parent = session_core.session(session.session_id).await.unwrap();
    assert_eq!(reloaded_parent.child_session_ids, vec![child.session_id]);

    let closed = session_core.close_session(session.session_id).await.unwrap();
    assert_eq!(closed.lifecycle_state, SessionLifecycleState::Closed);
    assert!(closed.closed_at.is_some());

    let attach_error = session_core.attach_session(session.session_id).await.unwrap_err();
    assert!(format!("{attach_error}").contains("closed"));
}

#[tokio::test]
async fn test_session_core_rejects_invalid_parent() {
    let session_core = SessionCore::new();

    let non_existent_parent = Uuid::new_v4();
    let fork_request = ForkSessionRequest {
        parent_session_id: non_existent_parent,
        attached_thread_id: None,
        local_policy: SessionPolicy::new(),
    };

    let result = session_core.fork_session(fork_request).await;
    assert!(result.is_err());
}

#[test]
fn test_session_context_operations() {
    let mut context = SessionContext::new();
    context.insert("key", "value");

    assert_eq!(context.get("key"), Some(&serde_json::Value::String("value".to_string())));
}

#[test]
fn test_session_policy_operations() {
    let mut policy = SessionPolicy::new();
    policy.insert("max_turns", 10);

    assert_eq!(policy.get("max_turns"), Some(&serde_json::Value::Number(10.into())));
}
