//! Wraps a [`ToolPort`] and appends [`ToolRecord`](state_abstraction::traits::ToolRecord) on each invoke.

use std::sync::Arc;

use agent_ports::{
    PortResult, RunId, ThreadId, ToolAssemblyPolicy, ToolCallSpec, ToolManifest, ToolPort,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use state_abstraction::traits::{ToolRecord, ToolRecordStore};

/// Persists each tool invocation to [`ToolRecordStore`] (FS/SQLite/etc.) for audit trails.
pub struct PersistingToolPort {
    inner: Arc<dyn ToolPort>,
    records: Arc<dyn ToolRecordStore>,
}

impl PersistingToolPort {
    #[must_use]
    pub fn new(inner: Arc<dyn ToolPort>, records: Arc<dyn ToolRecordStore>) -> Self {
        Self { inner, records }
    }
}

#[async_trait]
impl ToolPort for PersistingToolPort {
    fn manifests(&self) -> Vec<ToolManifest> {
        self.inner.manifests()
    }

    async fn invoke(
        &self,
        run_id: RunId,
        thread_id: ThreadId,
        call: &ToolCallSpec,
    ) -> PortResult<Value> {
        let res = self.inner.invoke(run_id, thread_id, call).await;
        let record_result = match &res {
            Ok(v) => v.clone(),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        let record = ToolRecord {
            thread_id: thread_id.0,
            tool_name: call.name.clone(),
            args: call.args.clone(),
            result: record_result,
            created_at: Utc::now(),
        };
        if let Err(e) = self.records.append_tool_record(&record).await {
            tracing::warn!(error = %e, tool = %call.name, "persist tool record failed");
        }
        res
    }

    fn assemble(&self, policy: &ToolAssemblyPolicy) -> Vec<ToolManifest> {
        self.inner.assemble(policy)
    }
}
