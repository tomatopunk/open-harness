use crate::traits::StateError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionContext {
    values: Map<String, Value>,
}

impl SessionContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionPolicy {
    values: Map<String, Value>,
}

impl SessionPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn merged_with(&self, local_override: &SessionPolicy) -> SessionPolicy {
        let mut values = self.values.clone();
        values.extend(local_override.values.clone());
        SessionPolicy { values }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSessionRequest {
    pub attached_thread_id: Option<Uuid>,
    pub context: SessionContext,
    pub policy: SessionPolicy,
}

impl Default for CreateSessionRequest {
    fn default() -> Self {
        Self {
            attached_thread_id: None,
            context: SessionContext::default(),
            policy: SessionPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSessionRequest {
    pub parent_session_id: Uuid,
    pub attached_thread_id: Option<Uuid>,
    pub local_policy: SessionPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecycleState {
    Active,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: Uuid,
    pub parent_session_id: Option<Uuid>,
    pub child_session_ids: Vec<Uuid>,
    pub attached_thread_id: Option<Uuid>,
    pub context: SessionContext,
    pub policy: SessionPolicy,
    pub local_policy: SessionPolicy,
    pub lifecycle_state: SessionLifecycleState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct SessionCore {
    sessions: RwLock<HashMap<Uuid, SessionRecord>>,
}

impl SessionCore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create_session(
        &self,
        request: CreateSessionRequest,
    ) -> Result<SessionRecord, StateError> {
        let now = Utc::now();
        let session = SessionRecord {
            session_id: Uuid::new_v4(),
            parent_session_id: None,
            child_session_ids: Vec::new(),
            attached_thread_id: request.attached_thread_id,
            context: request.context,
            policy: request.policy.clone(),
            local_policy: request.policy,
            lifecycle_state: SessionLifecycleState::Active,
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        self.sessions.write().await.insert(session.session_id, session.clone());
        Ok(session)
    }

    pub async fn attach_session(&self, session_id: Uuid) -> Result<SessionRecord, StateError> {
        let session = self.session(session_id).await?;
        if session.lifecycle_state == SessionLifecycleState::Closed {
            return Err(StateError::Conflict(format!(
                "session {session_id} is closed and cannot be attached"
            )));
        }

        Ok(session)
    }

    pub async fn fork_session(
        &self,
        request: ForkSessionRequest,
    ) -> Result<SessionRecord, StateError> {
        let mut sessions = self.sessions.write().await;
        let parent = sessions.get_mut(&request.parent_session_id).ok_or_else(|| {
            StateError::NotFound(format!(
                "parent session {} was not found",
                request.parent_session_id
            ))
        })?;

        if parent.lifecycle_state == SessionLifecycleState::Closed {
            return Err(StateError::Conflict(format!(
                "parent session {} is closed and cannot be forked",
                request.parent_session_id
            )));
        }

        let now = Utc::now();
        let child = SessionRecord {
            session_id: Uuid::new_v4(),
            parent_session_id: Some(parent.session_id),
            child_session_ids: Vec::new(),
            attached_thread_id: request.attached_thread_id.or(parent.attached_thread_id),
            context: parent.context.clone(),
            policy: parent.policy.merged_with(&request.local_policy),
            local_policy: request.local_policy,
            lifecycle_state: SessionLifecycleState::Active,
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        parent.child_session_ids.push(child.session_id);
        parent.updated_at = now;
        sessions.insert(child.session_id, child.clone());

        Ok(child)
    }

    pub async fn close_session(&self, session_id: Uuid) -> Result<SessionRecord, StateError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| StateError::NotFound(format!("session {session_id} was not found")))?;

        if session.lifecycle_state == SessionLifecycleState::Closed {
            return Ok(session.clone());
        }

        let now = Utc::now();
        session.lifecycle_state = SessionLifecycleState::Closed;
        session.updated_at = now;
        session.closed_at = Some(now);

        Ok(session.clone())
    }

    pub async fn session(&self, session_id: Uuid) -> Result<SessionRecord, StateError> {
        self.sessions
            .read()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| StateError::NotFound(format!("session {session_id} was not found")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn session_core_create_attach_and_close_root_session() {
        let core = SessionCore::new();
        let mut context = SessionContext::new();
        context.insert("channel", json!("manage"));

        let mut policy = SessionPolicy::new();
        policy.insert("mode", json!("safe"));

        let created = core
            .create_session(CreateSessionRequest {
                attached_thread_id: None,
                context: context.clone(),
                policy: policy.clone(),
            })
            .await
            .unwrap();

        assert_eq!(created.parent_session_id, None);
        assert_eq!(created.context, context);
        assert_eq!(created.policy, policy);
        assert_eq!(created.local_policy, policy);

        let attached = core.attach_session(created.session_id).await.unwrap();
        assert_eq!(attached.session_id, created.session_id);

        let closed = core.close_session(created.session_id).await.unwrap();
        assert_eq!(closed.lifecycle_state, SessionLifecycleState::Closed);
        assert!(closed.closed_at.is_some());

        let attach_error = core.attach_session(created.session_id).await.unwrap_err();
        assert!(matches!(attach_error, StateError::Conflict(_)));
    }

    #[tokio::test]
    async fn session_core_rejects_invalid_parent_reference() {
        let core = SessionCore::new();
        let error = core
            .fork_session(ForkSessionRequest {
                parent_session_id: Uuid::new_v4(),
                attached_thread_id: None,
                local_policy: SessionPolicy::default(),
            })
            .await
            .unwrap_err();

        assert!(matches!(error, StateError::NotFound(_)));
    }

    #[tokio::test]
    async fn session_core_fork_inherits_parent_context_and_overrides_policy() {
        let core = SessionCore::new();
        let thread_id = Uuid::new_v4();

        let mut context = SessionContext::new();
        context.insert("channel", json!("gateway"));
        context.insert("user", json!("murphy"));

        let mut parent_policy = SessionPolicy::new();
        parent_policy.insert("mode", json!("safe"));
        parent_policy.insert("max_iterations", json!(12));

        let parent = core
            .create_session(CreateSessionRequest {
                attached_thread_id: Some(thread_id),
                context: context.clone(),
                policy: parent_policy,
            })
            .await
            .unwrap();

        let mut local_policy = SessionPolicy::new();
        local_policy.insert("max_iterations", json!(3));
        local_policy.insert("sandbox", json!("restricted"));

        let child = core
            .fork_session(ForkSessionRequest {
                parent_session_id: parent.session_id,
                attached_thread_id: None,
                local_policy: local_policy.clone(),
            })
            .await
            .unwrap();

        assert_eq!(child.parent_session_id, Some(parent.session_id));
        assert_eq!(child.attached_thread_id, Some(thread_id));
        assert_eq!(child.context, context);
        assert_eq!(child.local_policy, local_policy);
        assert_eq!(child.policy.get("mode"), Some(&json!("safe")));
        assert_eq!(child.policy.get("max_iterations"), Some(&json!(3)));
        assert_eq!(child.policy.get("sandbox"), Some(&json!("restricted")));

        let updated_parent = core.session(parent.session_id).await.unwrap();
        assert_eq!(updated_parent.child_session_ids, vec![child.session_id]);
    }
}
