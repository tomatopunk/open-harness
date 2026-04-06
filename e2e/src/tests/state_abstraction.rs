use state_abstraction::{
    CreateSessionRequest, ForkSessionRequest, SessionContext, SessionCore, SessionPolicy,
};
use tokio;
use uuid::Uuid;

#[tokio::test]
async fn test_session_core_create_and_close() {
    let session_core = SessionCore::new();

    let create_request = CreateSessionRequest {
        attached_thread_id: None,
        context: SessionContext::new(),
        policy: SessionPolicy::new(),
    };

    let result = session_core.create_session(create_request).await;
    assert!(result.is_ok());

    let session = result.unwrap();
    assert!(session.parent_session_id.is_none());

    let close_result = session_core.close_session(session.session_id).await;
    assert!(close_result.is_ok());
}

#[tokio::test]
async fn test_session_core_fork() {
    let session_core = SessionCore::new();

    let create_request = CreateSessionRequest {
        attached_thread_id: None,
        context: SessionContext::new(),
        policy: SessionPolicy::new(),
    };

    let parent_session = session_core.create_session(create_request).await.unwrap();

    let fork_request = ForkSessionRequest {
        parent_session_id: parent_session.session_id,
        attached_thread_id: None,
        local_policy: SessionPolicy::new(),
    };

    let result = session_core.fork_session(fork_request).await;
    assert!(result.is_ok());

    let forked_session = result.unwrap();
    assert_eq!(forked_session.parent_session_id, Some(parent_session.session_id));
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
