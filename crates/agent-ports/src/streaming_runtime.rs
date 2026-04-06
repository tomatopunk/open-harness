use crate::error::{PortError, PortResult};
use crate::ids::{RunId, ThreadId};
use crate::tool_manifest::ToolProviderType;
use crate::tool_provider::ToolResult;
use crate::ToolCallSpec;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAdapterKind {
    File,
    Bash,
    AgentFork,
    Mcp,
    Skill,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicyAction {
    Allow,
    Downgrade,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSandboxProfile {
    Standard,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BashCommandRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashCommandClassification {
    pub risk: BashCommandRisk,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionSecurityContext {
    pub policy_action: ExecutionPolicyAction,
    pub policy_reason: String,
    pub sandbox_profile: ProcessSandboxProfile,
    pub bash_classification: Option<BashCommandClassification>,
}

#[derive(Debug, Clone)]
pub struct ToolRuntimeRequest {
    pub session_id: Uuid,
    pub run_id: RunId,
    pub thread_id: ThreadId,
    pub adapter_kind: ToolAdapterKind,
    pub provider_type: ToolProviderType,
    pub provider_name: String,
    pub call: ToolCallSpec,
    pub security: Option<ToolExecutionSecurityContext>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolStreamChunk {
    pub sequence: u64,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub enum ToolRuntimeEvent {
    Request { request: ToolRuntimeRequest },
    StreamChunk { call_id: String, chunk: ToolStreamChunk },
    Finalize { call_id: String, result: ToolResult },
    Error { call_id: String, error: String },
}

pub trait ToolRuntimeChunkSink: Send {
    fn emit(&mut self, payload: Value) -> PortResult<()>;
}

#[async_trait]
pub trait StreamingToolAdapter: Send + Sync {
    fn kind(&self) -> ToolAdapterKind;

    fn provider_name(&self) -> &str;

    async fn invoke(
        &self,
        request: &ToolRuntimeRequest,
        sink: &mut dyn ToolRuntimeChunkSink,
    ) -> PortResult<ToolResult>;
}

#[derive(Default)]
pub struct StreamingToolRuntime {
    adapters: RwLock<HashMap<ToolAdapterKind, Arc<dyn StreamingToolAdapter>>>,
}

impl StreamingToolRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_adapter(&self, adapter: Arc<dyn StreamingToolAdapter>) {
        self.adapters.write().await.insert(adapter.kind(), adapter);
    }

    pub async fn adapter_kinds(&self) -> Vec<ToolAdapterKind> {
        let mut kinds = self.adapters.read().await.keys().copied().collect::<Vec<_>>();
        kinds.sort_by_key(|kind| adapter_sort_order(*kind));
        kinds
    }

    pub async fn execute(
        &self,
        request: ToolRuntimeRequest,
    ) -> PortResult<mpsc::Receiver<ToolRuntimeEvent>> {
        let adapter = {
            self.adapters.read().await.get(&request.adapter_kind).cloned().ok_or_else(|| {
                PortError::NotFound(format!(
                    "no streaming adapter registered for {:?}",
                    request.adapter_kind
                ))
            })?
        };

        let (sender, receiver) = mpsc::channel(16);
        tokio::spawn(async move {
            let call_id = request.call.call_id.clone();
            if sender.send(ToolRuntimeEvent::Request { request: request.clone() }).await.is_err() {
                return;
            }

            let mut sink = ChannelToolRuntimeChunkSink::new(call_id.clone(), sender.clone());
            match adapter.invoke(&request, &mut sink).await {
                Ok(result) => {
                    let _ = sender.send(ToolRuntimeEvent::Finalize { call_id, result }).await;
                }
                Err(error) => {
                    let _ = sender
                        .send(ToolRuntimeEvent::Error { call_id, error: error.to_string() })
                        .await;
                }
            }
        });

        Ok(receiver)
    }
}

struct ChannelToolRuntimeChunkSink {
    call_id: String,
    sequence: u64,
    sender: mpsc::Sender<ToolRuntimeEvent>,
}

impl ChannelToolRuntimeChunkSink {
    fn new(call_id: String, sender: mpsc::Sender<ToolRuntimeEvent>) -> Self {
        Self { call_id, sequence: 0, sender }
    }
}

impl ToolRuntimeChunkSink for ChannelToolRuntimeChunkSink {
    fn emit(&mut self, payload: Value) -> PortResult<()> {
        let event = ToolRuntimeEvent::StreamChunk {
            call_id: self.call_id.clone(),
            chunk: ToolStreamChunk { sequence: self.sequence, payload },
        };
        self.sequence = self.sequence.saturating_add(1);

        self.sender
            .try_send(event)
            .map_err(|error| PortError::Tool(format!("failed to emit runtime chunk: {error}")))
    }
}

fn adapter_sort_order(kind: ToolAdapterKind) -> u8 {
    match kind {
        ToolAdapterKind::File => 0,
        ToolAdapterKind::Bash => 1,
        ToolAdapterKind::AgentFork => 2,
        ToolAdapterKind::Mcp => 3,
        ToolAdapterKind::Skill => 4,
        ToolAdapterKind::Web => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_provider::ToolResultMetadata;
    use serde_json::json;

    struct HappyPathAdapter;

    #[async_trait]
    impl StreamingToolAdapter for HappyPathAdapter {
        fn kind(&self) -> ToolAdapterKind {
            ToolAdapterKind::File
        }

        fn provider_name(&self) -> &str {
            "file"
        }

        async fn invoke(
            &self,
            request: &ToolRuntimeRequest,
            sink: &mut dyn ToolRuntimeChunkSink,
        ) -> PortResult<ToolResult> {
            sink.emit(json!({"part": "alpha", "tool": request.call.name}))?;
            sink.emit(json!({"part": "beta"}))?;
            Ok(ToolResult {
                success: true,
                data: json!({"status": "ok"}),
                metadata: ToolResultMetadata {
                    tool_name: request.call.name.clone(),
                    provider_type: request.provider_type,
                    provider_name: request.provider_name.clone(),
                    execution_time_ms: 12,
                    retries: 0,
                    error_message: None,
                },
            })
        }
    }

    struct ErrorAdapter;

    #[async_trait]
    impl StreamingToolAdapter for ErrorAdapter {
        fn kind(&self) -> ToolAdapterKind {
            ToolAdapterKind::Bash
        }

        fn provider_name(&self) -> &str {
            "bash"
        }

        async fn invoke(
            &self,
            _request: &ToolRuntimeRequest,
            sink: &mut dyn ToolRuntimeChunkSink,
        ) -> PortResult<ToolResult> {
            sink.emit(json!({"stderr": "partial output"}))?;
            Err(PortError::Tool("adapter failed".to_string()))
        }
    }

    fn test_request(kind: ToolAdapterKind, provider_name: &str) -> ToolRuntimeRequest {
        ToolRuntimeRequest {
            session_id: Uuid::nil(),
            run_id: RunId::new_v4(),
            thread_id: ThreadId::new_v4(),
            adapter_kind: kind,
            provider_type: ToolProviderType::Local,
            provider_name: provider_name.to_string(),
            call: ToolCallSpec {
                name: format!("{provider_name}-tool"),
                args: json!({"path": "README.md"}),
                call_id: format!("call-{provider_name}"),
            },
            security: None,
        }
    }

    async fn collect_events(
        mut receiver: mpsc::Receiver<ToolRuntimeEvent>,
    ) -> Vec<ToolRuntimeEvent> {
        let mut events = Vec::new();
        while let Some(event) = receiver.recv().await {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn streaming_runtime_happy_path_emits_request_chunk_and_finalize_sequence() {
        let runtime = StreamingToolRuntime::new();
        runtime.register_adapter(Arc::new(HappyPathAdapter)).await;

        let events = collect_events(
            runtime.execute(test_request(ToolAdapterKind::File, "file")).await.unwrap(),
        )
        .await;

        assert!(
            matches!(&events[0], ToolRuntimeEvent::Request { request } if request.adapter_kind == ToolAdapterKind::File)
        );
        assert!(
            matches!(&events[1], ToolRuntimeEvent::StreamChunk { chunk, .. } if chunk.sequence == 0)
        );
        assert!(
            matches!(&events[2], ToolRuntimeEvent::StreamChunk { chunk, .. } if chunk.sequence == 1)
        );
        assert!(matches!(&events[3], ToolRuntimeEvent::Finalize { result, .. } if result.success));
    }

    #[tokio::test]
    async fn streaming_runtime_adapter_error_becomes_terminal_error_event() {
        let runtime = StreamingToolRuntime::new();
        runtime.register_adapter(Arc::new(ErrorAdapter)).await;

        let events = collect_events(
            runtime.execute(test_request(ToolAdapterKind::Bash, "bash")).await.unwrap(),
        )
        .await;

        assert!(matches!(&events[0], ToolRuntimeEvent::Request { .. }));
        assert!(matches!(&events[1], ToolRuntimeEvent::StreamChunk { .. }));
        assert!(
            matches!(&events[2], ToolRuntimeEvent::Error { error, .. } if error.contains("adapter failed"))
        );
        assert_eq!(events.len(), 3);
    }
}
