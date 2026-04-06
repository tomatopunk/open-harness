use crate::{AgentKernel, KernelError, KernelEvent, KernelResult};
use agent_ports::{
    PortResult, StreamingToolAdapter, ToolAdapterKind, ToolResult, ToolRuntimeChunkSink,
    ToolRuntimeEvent, ToolRuntimeRequest,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

#[async_trait]
pub trait ToolAdapterExecutor: Send + Sync {
    async fn execute(
        &self,
        request: &ToolRuntimeRequest,
        sink: &mut dyn ToolRuntimeChunkSink,
    ) -> PortResult<ToolResult>;
}

macro_rules! define_streaming_adapter {
    ($name:ident, $kind:expr, $provider_type:expr) => {
        pub struct $name {
            provider_name: String,
            executor: Arc<dyn ToolAdapterExecutor>,
        }

        impl $name {
            #[must_use]
            pub fn new(
                provider_name: impl Into<String>,
                executor: Arc<dyn ToolAdapterExecutor>,
            ) -> Self {
                Self { provider_name: provider_name.into(), executor }
            }
        }

        #[async_trait]
        impl StreamingToolAdapter for $name {
            fn kind(&self) -> ToolAdapterKind {
                $kind
            }

            fn provider_name(&self) -> &str {
                &self.provider_name
            }

            async fn invoke(
                &self,
                request: &ToolRuntimeRequest,
                sink: &mut dyn ToolRuntimeChunkSink,
            ) -> PortResult<ToolResult> {
                debug_assert_eq!(request.adapter_kind, $kind);
                debug_assert_eq!(request.provider_type, $provider_type);
                self.executor.execute(request, sink).await
            }
        }
    };
}

define_streaming_adapter!(
    FileToolAdapter,
    ToolAdapterKind::File,
    agent_ports::ToolProviderType::Local
);
define_streaming_adapter!(
    BashToolAdapter,
    ToolAdapterKind::Bash,
    agent_ports::ToolProviderType::Local
);
define_streaming_adapter!(
    AgentForkToolAdapter,
    ToolAdapterKind::AgentFork,
    agent_ports::ToolProviderType::Local
);
define_streaming_adapter!(McpToolAdapter, ToolAdapterKind::Mcp, agent_ports::ToolProviderType::Mcp);
define_streaming_adapter!(
    SkillToolAdapter,
    ToolAdapterKind::Skill,
    agent_ports::ToolProviderType::Skill
);
define_streaming_adapter!(
    WebToolAdapter,
    ToolAdapterKind::Web,
    agent_ports::ToolProviderType::Community
);

impl AgentKernel {
    pub async fn register_tool_adapter(&self, adapter: Arc<dyn StreamingToolAdapter>) {
        self.tool_runtime().register_adapter(adapter).await;
    }

    pub async fn execute_tool_runtime(
        &self,
        request: ToolRuntimeRequest,
    ) -> KernelResult<mpsc::Receiver<ToolRuntimeEvent>> {
        self.transition_tool_execution(KernelEvent::ToolExecutionStarted {
            session_id: request.session_id,
            tool_call_id: request.call.call_id.clone(),
            tool_name: request.call.name.clone(),
        })
        .await?;

        let inner_receiver = match self.tool_runtime().execute(request.clone()).await {
            Ok(receiver) => receiver,
            Err(error) => {
                self.finish_tool_runtime_transition(&request, false).await?;
                return Err(KernelError::Lifecycle(format!(
                    "Failed to start tool runtime for {} via {:?}: {}",
                    request.call.name, request.adapter_kind, error
                )));
            }
        };

        let (outer_sender, outer_receiver) = mpsc::channel(16);
        let kernel = self.clone();
        tokio::spawn(async move {
            relay_runtime_events(kernel, request, inner_receiver, outer_sender).await;
        });

        Ok(outer_receiver)
    }

    async fn finish_tool_runtime_transition(
        &self,
        request: &ToolRuntimeRequest,
        success: bool,
    ) -> KernelResult<()> {
        self.transition_tool_execution(KernelEvent::ToolExecutionFinished {
            session_id: request.session_id,
            tool_call_id: request.call.call_id.clone(),
            tool_name: request.call.name.clone(),
            success,
        })
        .await
    }
}

async fn relay_runtime_events(
    kernel: AgentKernel,
    request: ToolRuntimeRequest,
    mut inner_receiver: mpsc::Receiver<ToolRuntimeEvent>,
    outer_sender: mpsc::Sender<ToolRuntimeEvent>,
) {
    while let Some(event) = inner_receiver.recv().await {
        let terminal_success = match &event {
            ToolRuntimeEvent::Finalize { .. } => Some(true),
            ToolRuntimeEvent::Error { .. } => Some(false),
            ToolRuntimeEvent::Request { .. } | ToolRuntimeEvent::StreamChunk { .. } => None,
        };

        if outer_sender.send(event).await.is_err() {
            break;
        }

        if let Some(success) = terminal_success {
            let _ = kernel.finish_tool_runtime_transition(&request, success).await;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KernelConfig, KernelState};
    use agent_ports::{
        RunId, ThreadId, ToolCallSpec, ToolProviderType, ToolResultMetadata, ToolRuntimeEvent,
    };
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct RecordingExecutor {
        payload_label: &'static str,
        should_fail: bool,
    }

    #[async_trait]
    impl ToolAdapterExecutor for RecordingExecutor {
        async fn execute(
            &self,
            request: &ToolRuntimeRequest,
            sink: &mut dyn ToolRuntimeChunkSink,
        ) -> PortResult<ToolResult> {
            sink.emit(json!({"adapter": self.payload_label, "phase": "stream"}))?;
            if self.should_fail {
                return Err(agent_ports::PortError::Tool(format!(
                    "{} executor failed",
                    self.payload_label
                )));
            }

            Ok(ToolResult {
                success: true,
                data: json!({"adapter": self.payload_label, "tool": request.call.name}),
                metadata: ToolResultMetadata {
                    tool_name: request.call.name.clone(),
                    provider_type: request.provider_type,
                    provider_name: request.provider_name.clone(),
                    execution_time_ms: 5,
                    retries: 0,
                    error_message: None,
                },
            })
        }
    }

    fn create_workspace_root(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root =
            std::env::temp_dir().join(format!("streaming-runtime-tests-{name}-{now}-{suffix}"));
        fs::create_dir_all(root.join("plugins")).unwrap();
        root
    }

    fn test_kernel(name: &str) -> AgentKernel {
        let workspace_root = create_workspace_root(name);
        let mut config = KernelConfig::default();
        config.workspace_root = workspace_root.clone();
        config.plugins_dir = workspace_root.join("plugins");
        AgentKernel::new(config)
    }

    fn runtime_request(
        adapter_kind: ToolAdapterKind,
        provider_type: ToolProviderType,
        provider_name: &str,
    ) -> ToolRuntimeRequest {
        ToolRuntimeRequest {
            session_id: uuid::Uuid::nil(),
            run_id: RunId::new_v4(),
            thread_id: ThreadId::new_v4(),
            adapter_kind,
            provider_type,
            provider_name: provider_name.to_string(),
            call: ToolCallSpec {
                name: format!("{provider_name}-tool"),
                args: json!({"value": provider_name}),
                call_id: format!("call-{provider_name}"),
            },
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
    async fn streaming_runtime_shared_bus_exposes_all_adapter_classes() {
        let kernel = test_kernel("all-adapters");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();

        let local = Arc::new(RecordingExecutor { payload_label: "local", should_fail: false });
        let mcp = Arc::new(RecordingExecutor { payload_label: "mcp", should_fail: false });
        let skill = Arc::new(RecordingExecutor { payload_label: "skill", should_fail: false });
        let web = Arc::new(RecordingExecutor { payload_label: "web", should_fail: false });

        kernel.register_tool_adapter(Arc::new(FileToolAdapter::new("file", local.clone()))).await;
        kernel.register_tool_adapter(Arc::new(BashToolAdapter::new("bash", local.clone()))).await;
        kernel.register_tool_adapter(Arc::new(AgentForkToolAdapter::new("agent", local))).await;
        kernel.register_tool_adapter(Arc::new(McpToolAdapter::new("mcp", mcp))).await;
        kernel.register_tool_adapter(Arc::new(SkillToolAdapter::new("skill", skill))).await;
        kernel.register_tool_adapter(Arc::new(WebToolAdapter::new("web", web))).await;

        assert_eq!(
            kernel.tool_runtime().adapter_kinds().await,
            vec![
                ToolAdapterKind::File,
                ToolAdapterKind::Bash,
                ToolAdapterKind::AgentFork,
                ToolAdapterKind::Mcp,
                ToolAdapterKind::Skill,
                ToolAdapterKind::Web,
            ]
        );

        let cases = vec![
            runtime_request(ToolAdapterKind::File, ToolProviderType::Local, "file"),
            runtime_request(ToolAdapterKind::Bash, ToolProviderType::Local, "bash"),
            runtime_request(ToolAdapterKind::AgentFork, ToolProviderType::Local, "agent"),
            runtime_request(ToolAdapterKind::Mcp, ToolProviderType::Mcp, "mcp"),
            runtime_request(ToolAdapterKind::Skill, ToolProviderType::Skill, "skill"),
            runtime_request(ToolAdapterKind::Web, ToolProviderType::Community, "web"),
        ];

        for case in cases {
            let events =
                collect_events(kernel.execute_tool_runtime(case.clone()).await.unwrap()).await;
            assert!(
                matches!(&events[0], ToolRuntimeEvent::Request { request } if request.adapter_kind == case.adapter_kind)
            );
            assert!(matches!(&events[1], ToolRuntimeEvent::StreamChunk { .. }));
            assert!(
                matches!(&events[2], ToolRuntimeEvent::Finalize { result, .. } if result.success)
            );
        }

        assert_eq!(kernel.current_state().await, KernelState::Running);
    }

    #[tokio::test]
    async fn streaming_runtime_kernel_marks_terminal_error_without_leaving_running_state() {
        let kernel = test_kernel("error-terminal");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();
        kernel
            .register_tool_adapter(Arc::new(BashToolAdapter::new(
                "bash",
                Arc::new(RecordingExecutor { payload_label: "bash", should_fail: true }),
            )))
            .await;

        let events = collect_events(
            kernel
                .execute_tool_runtime(runtime_request(
                    ToolAdapterKind::Bash,
                    ToolProviderType::Local,
                    "bash",
                ))
                .await
                .unwrap(),
        )
        .await;

        assert!(matches!(&events[0], ToolRuntimeEvent::Request { .. }));
        assert!(matches!(&events[1], ToolRuntimeEvent::StreamChunk { .. }));
        assert!(
            matches!(&events[2], ToolRuntimeEvent::Error { error, .. } if error.contains("executor failed"))
        );
        assert_eq!(kernel.current_state().await, KernelState::Running);
    }
}
