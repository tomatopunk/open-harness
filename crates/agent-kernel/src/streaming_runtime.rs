use crate::{AgentKernel, KernelError, KernelEvent, KernelResult};
use agent_ports::{
    ExecutionPolicyAction, PortResult, StreamingToolAdapter, ToolAdapterKind, ToolResult,
    ToolRuntimeChunkSink, ToolRuntimeEvent, ToolRuntimeRequest,
};
use async_trait::async_trait;
use serde_json::Value;
use state_abstraction::{SandboxExecution, SessionRecord};
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
        let session = self.attach_session(request.session_id).await.map_err(|error| {
            KernelError::context(
                format!(
                    "Failed to attach execution session {} for tool {}",
                    request.session_id, request.call.name
                ),
                error,
            )
        })?;
        let evaluation = self.security_chain().evaluate(&request, &session);
        let mut secured_request = request;
        secured_request.security = Some(evaluation.context.clone());

        self.transition_tool_execution(KernelEvent::ToolExecutionStarted {
            session_id: secured_request.session_id,
            tool_call_id: secured_request.call.call_id.clone(),
            tool_name: secured_request.call.name.clone(),
        })
        .await?;

        if evaluation.context.policy_action == ExecutionPolicyAction::Block {
            let audit = build_audit_record(
                &secured_request,
                &session,
                evaluation.command,
                "blocked",
                false,
                String::new(),
                evaluation.context.policy_reason.clone(),
            );
            self.execution_audit_store().append_execution(&audit).await.map_err(|error| {
                KernelError::context(
                    format!(
                        "Failed to persist blocked execution audit for {}",
                        secured_request.call.name
                    ),
                    error,
                )
            })?;

            self.finish_tool_runtime_transition(&secured_request, false).await?;
            return Ok(blocked_runtime_receiver(secured_request, evaluation.context.policy_reason));
        }

        let inner_receiver = match self.tool_runtime().execute(secured_request.clone()).await {
            Ok(receiver) => receiver,
            Err(error) => {
                let audit = build_audit_record(
                    &secured_request,
                    &session,
                    evaluation.command,
                    "startup_error",
                    false,
                    String::new(),
                    error.to_string(),
                );
                self.execution_audit_store().append_execution(&audit).await.map_err(
                    |audit_error| {
                        KernelError::context(
                            format!(
                                "Failed to persist startup audit for {}",
                                secured_request.call.name
                            ),
                            audit_error,
                        )
                    },
                )?;
                self.finish_tool_runtime_transition(&secured_request, false).await?;
                return Err(KernelError::Lifecycle(format!(
                    "Failed to start tool runtime for {} via {:?}: {}",
                    secured_request.call.name, secured_request.adapter_kind, error
                )));
            }
        };

        let (outer_sender, outer_receiver) = mpsc::channel(16);
        let kernel = self.clone();
        tokio::spawn(async move {
            relay_runtime_events(
                kernel,
                secured_request,
                session,
                evaluation.command,
                inner_receiver,
                outer_sender,
            )
            .await;
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
    session: SessionRecord,
    command: Option<String>,
    mut inner_receiver: mpsc::Receiver<ToolRuntimeEvent>,
    outer_sender: mpsc::Sender<ToolRuntimeEvent>,
) {
    while let Some(event) = inner_receiver.recv().await {
        let terminal = match &event {
            ToolRuntimeEvent::Finalize { result, .. } => Some((
                true,
                build_audit_record(
                    &request,
                    &session,
                    command.clone(),
                    "finalized",
                    result.success,
                    serialize_result_payload(&result.data),
                    result.metadata.error_message.clone().unwrap_or_default(),
                ),
            )),
            ToolRuntimeEvent::Error { error, .. } => Some((
                false,
                build_audit_record(
                    &request,
                    &session,
                    command.clone(),
                    "error",
                    false,
                    String::new(),
                    error.clone(),
                ),
            )),
            ToolRuntimeEvent::Request { .. } | ToolRuntimeEvent::StreamChunk { .. } => None,
        };

        let event_to_send = if let Some((_, audit)) = &terminal {
            match kernel.execution_audit_store().append_execution(audit).await {
                Ok(()) => event,
                Err(error) => ToolRuntimeEvent::Error {
                    call_id: request.call.call_id.clone(),
                    error: format!("security audit failed: {error}"),
                },
            }
        } else {
            event
        };

        let terminal_success = match &event_to_send {
            ToolRuntimeEvent::Finalize { .. } => Some(true),
            ToolRuntimeEvent::Error { .. } => Some(false),
            ToolRuntimeEvent::Request { .. } | ToolRuntimeEvent::StreamChunk { .. } => None,
        };

        if outer_sender.send(event_to_send).await.is_err() {
            break;
        }

        if let Some(success) = terminal_success {
            let _ = kernel.finish_tool_runtime_transition(&request, success).await;
            break;
        }
    }
}

fn blocked_runtime_receiver(
    request: ToolRuntimeRequest,
    reason: String,
) -> mpsc::Receiver<ToolRuntimeEvent> {
    let (sender, receiver) = mpsc::channel(4);
    tokio::spawn(async move {
        if sender.send(ToolRuntimeEvent::Request { request: request.clone() }).await.is_err() {
            return;
        }
        let _ = sender
            .send(ToolRuntimeEvent::Error { call_id: request.call.call_id.clone(), error: reason })
            .await;
    });
    receiver
}

fn build_audit_record(
    request: &ToolRuntimeRequest,
    session: &SessionRecord,
    command: Option<String>,
    outcome: &str,
    success: bool,
    stdout: String,
    stderr: String,
) -> SandboxExecution {
    let security = request.security.clone().expect("secured request must include security context");
    SandboxExecution::new(
        uuid::Uuid::new_v4(),
        session.session_id,
        request.thread_id.0,
        request.call.call_id.clone(),
        request.call.name.clone(),
        request.adapter_kind,
        request.provider_name.clone(),
        command,
        security.policy_action,
        security.policy_reason,
        security.sandbox_profile,
        security.bash_classification.as_ref().map(|classification| classification.risk),
        security.bash_classification.as_ref().map(|classification| classification.reason.clone()),
        outcome.to_string(),
        success,
        None,
        stdout,
        stderr,
    )
}

fn serialize_result_payload(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LocalFsConfig;
    use crate::{CreateSessionRequest, KernelConfig, KernelState, SessionContext, SessionPolicy};
    use agent_ports::{
        BashCommandRisk, ExecutionPolicyAction, ProcessSandboxProfile, RunId, ThreadId,
        ToolCallSpec, ToolProviderType, ToolResultMetadata, ToolRuntimeEvent,
    };
    use serde_json::json;
    use state_abstraction::{
        memory_document::FactCategory, LocalFsStateStore, MemoryDocument, SandboxExecutionStore,
    };
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    struct RecordingExecutor {
        payload_label: &'static str,
        should_fail: bool,
        seen_requests: Arc<Mutex<Vec<ToolRuntimeRequest>>>,
    }

    #[async_trait]
    impl ToolAdapterExecutor for RecordingExecutor {
        async fn execute(
            &self,
            request: &ToolRuntimeRequest,
            sink: &mut dyn ToolRuntimeChunkSink,
        ) -> PortResult<ToolResult> {
            self.seen_requests.lock().await.push(request.clone());
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
        let config = KernelConfig {
            workspace_root: workspace_root.clone(),
            plugins_dir: workspace_root.join("plugins"),
            storage: crate::config::StorageConfig {
                local_fs: Some(LocalFsConfig { root: std::path::PathBuf::from(".data/local-fs") }),
                ..Default::default()
            },
            ..Default::default()
        };
        AgentKernel::new(config)
    }

    async fn test_session(kernel: &AgentKernel, policy: SessionPolicy) -> Uuid {
        kernel
            .create_session(CreateSessionRequest {
                attached_thread_id: None,
                context: SessionContext::new(),
                policy,
            })
            .await
            .unwrap()
            .session_id
    }

    fn runtime_request(
        session_id: Uuid,
        adapter_kind: ToolAdapterKind,
        provider_type: ToolProviderType,
        provider_name: &str,
        args: serde_json::Value,
    ) -> ToolRuntimeRequest {
        ToolRuntimeRequest {
            session_id,
            run_id: RunId::new_v4(),
            thread_id: ThreadId::new_v4(),
            adapter_kind,
            provider_type,
            provider_name: provider_name.to_string(),
            call: ToolCallSpec {
                name: format!("{provider_name}-tool"),
                args,
                call_id: format!("call-{provider_name}"),
            },
            security: None,
        }
    }

    fn audit_store(kernel: &AgentKernel) -> LocalFsStateStore {
        let root = kernel
            .config()
            .storage
            .local_fs
            .as_ref()
            .map(|local_fs| kernel.config().workspace_root.join(&local_fs.root))
            .unwrap();
        LocalFsStateStore::new(root)
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

        let local_requests = Arc::new(Mutex::new(Vec::new()));
        let local = Arc::new(RecordingExecutor {
            payload_label: "local",
            should_fail: false,
            seen_requests: local_requests.clone(),
        });
        let mcp = Arc::new(RecordingExecutor {
            payload_label: "mcp",
            should_fail: false,
            seen_requests: Arc::new(Mutex::new(Vec::new())),
        });
        let skill = Arc::new(RecordingExecutor {
            payload_label: "skill",
            should_fail: false,
            seen_requests: Arc::new(Mutex::new(Vec::new())),
        });
        let web = Arc::new(RecordingExecutor {
            payload_label: "web",
            should_fail: false,
            seen_requests: Arc::new(Mutex::new(Vec::new())),
        });

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
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::File,
                ToolProviderType::Local,
                "file",
                json!({"path": "README.md"}),
            ),
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::Bash,
                ToolProviderType::Local,
                "bash",
                json!({"command": "pwd"}),
            ),
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::AgentFork,
                ToolProviderType::Local,
                "agent",
                json!({"task": "delegate"}),
            ),
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::Mcp,
                ToolProviderType::Mcp,
                "mcp",
                json!({"server": "demo"}),
            ),
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::Skill,
                ToolProviderType::Skill,
                "skill",
                json!({"name": "lint"}),
            ),
            runtime_request(
                test_session(&kernel, SessionPolicy::new()).await,
                ToolAdapterKind::Web,
                ToolProviderType::Community,
                "web",
                json!({"url": "https://example.com"}),
            ),
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

        assert!(local_requests.lock().await.iter().all(|request| request.security.is_some()));
        assert_eq!(kernel.current_state().await, KernelState::Running);
    }

    #[tokio::test]
    async fn streaming_runtime_kernel_marks_terminal_error_without_leaving_running_state() {
        let kernel = test_kernel("error-terminal");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();
        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        kernel
            .register_tool_adapter(Arc::new(BashToolAdapter::new(
                "bash",
                Arc::new(RecordingExecutor {
                    payload_label: "bash",
                    should_fail: true,
                    seen_requests: seen_requests.clone(),
                }),
            )))
            .await;

        let session_id = test_session(&kernel, SessionPolicy::new()).await;

        let events = collect_events(
            kernel
                .execute_tool_runtime(runtime_request(
                    session_id,
                    ToolAdapterKind::Bash,
                    ToolProviderType::Local,
                    "bash",
                    json!({"command": "pwd"}),
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
        let seen = seen_requests.lock().await;
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].security.as_ref().unwrap().policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(kernel.current_state().await, KernelState::Running);
    }

    #[tokio::test]
    async fn harmless_bash_command_is_allowed_and_audited() {
        let kernel = test_kernel("harmless-audit");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();

        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        kernel
            .register_tool_adapter(Arc::new(BashToolAdapter::new(
                "bash",
                Arc::new(RecordingExecutor {
                    payload_label: "bash",
                    should_fail: false,
                    seen_requests: seen_requests.clone(),
                }),
            )))
            .await;

        let session_id = test_session(&kernel, SessionPolicy::new()).await;
        let request = runtime_request(
            session_id,
            ToolAdapterKind::Bash,
            ToolProviderType::Local,
            "bash",
            json!({"command": "pwd"}),
        );
        let thread_id = request.thread_id.0;

        let events = collect_events(kernel.execute_tool_runtime(request).await.unwrap()).await;

        assert!(matches!(&events[2], ToolRuntimeEvent::Finalize { result, .. } if result.success));
        let seen = seen_requests.lock().await;
        assert_eq!(seen.len(), 1);
        let security = seen[0].security.as_ref().unwrap();
        assert_eq!(security.policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(security.sandbox_profile, ProcessSandboxProfile::Standard);

        let audits = audit_store(&kernel).list_executions(thread_id).await.unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(audits[0].outcome, "finalized");
        assert!(audits[0].success);
        assert_eq!(audits[0].command.as_deref(), Some("pwd"));
    }

    #[tokio::test]
    async fn dangerous_bash_command_is_blocked_before_executor_and_audited() {
        let kernel = test_kernel("dangerous-blocked");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();

        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        kernel
            .register_tool_adapter(Arc::new(BashToolAdapter::new(
                "bash",
                Arc::new(RecordingExecutor {
                    payload_label: "bash",
                    should_fail: false,
                    seen_requests: seen_requests.clone(),
                }),
            )))
            .await;

        let session_id = test_session(&kernel, SessionPolicy::new()).await;
        let request = runtime_request(
            session_id,
            ToolAdapterKind::Bash,
            ToolProviderType::Local,
            "bash",
            json!({"command": "rm -rf / --no-preserve-root"}),
        );
        let thread_id = request.thread_id.0;

        let events = collect_events(kernel.execute_tool_runtime(request).await.unwrap()).await;

        assert!(
            matches!(&events[0], ToolRuntimeEvent::Request { request } if request.security.as_ref().unwrap().policy_action == ExecutionPolicyAction::Block)
        );
        assert!(
            matches!(&events[1], ToolRuntimeEvent::Error { error, .. } if error.contains("blocked high-risk command"))
        );
        assert!(seen_requests.lock().await.is_empty());

        let audits = audit_store(&kernel).list_executions(thread_id).await.unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].policy_action, ExecutionPolicyAction::Block);
        assert_eq!(audits[0].outcome, "blocked");
        assert!(!audits[0].success);
        assert_eq!(audits[0].classifier_risk, Some(BashCommandRisk::High));
        assert!(audits[0].stderr.contains("blocked high-risk command"));
    }

    #[tokio::test]
    async fn runtime_execution_path_always_injects_security_context_before_adapter_invocation() {
        let kernel = test_kernel("security-no-bypass");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();

        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        kernel
            .register_tool_adapter(Arc::new(FileToolAdapter::new(
                "file",
                Arc::new(RecordingExecutor {
                    payload_label: "file",
                    should_fail: false,
                    seen_requests: seen_requests.clone(),
                }),
            )))
            .await;

        let mut policy = SessionPolicy::new();
        policy.insert("sandbox", json!("restricted"));
        let session_id = test_session(&kernel, policy).await;

        let events = collect_events(
            kernel
                .execute_tool_runtime(runtime_request(
                    session_id,
                    ToolAdapterKind::File,
                    ToolProviderType::Local,
                    "file",
                    json!({"path": "README.md"}),
                ))
                .await
                .unwrap(),
        )
        .await;

        assert!(matches!(&events[2], ToolRuntimeEvent::Finalize { .. }));
        let seen = seen_requests.lock().await;
        assert_eq!(seen.len(), 1);
        let security = seen[0].security.as_ref().unwrap();
        assert_eq!(security.policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(security.sandbox_profile, ProcessSandboxProfile::Restricted);
        assert!(security
            .policy_reason
            .contains("session policy requested restricted process sandbox"));
    }

    #[tokio::test]
    async fn session_runtime_security_and_memory_chain_stays_coherent() {
        let kernel = test_kernel("engine-certification-chain");
        kernel.initialize().await.unwrap();
        kernel.start().await.unwrap();

        let seen_requests = Arc::new(Mutex::new(Vec::new()));
        kernel
            .register_tool_adapter(Arc::new(FileToolAdapter::new(
                "file",
                Arc::new(RecordingExecutor {
                    payload_label: "file",
                    should_fail: false,
                    seen_requests: seen_requests.clone(),
                }),
            )))
            .await;

        let thread_id = Uuid::new_v4();
        let mut context = SessionContext::new();
        context.insert("channel", json!("gateway"));
        context.insert("request_id", json!("cert-001"));

        let mut policy = SessionPolicy::new();
        policy.insert("sandbox", json!("restricted"));
        policy.insert("mode", json!("safe"));

        let session = kernel
            .create_session(CreateSessionRequest {
                attached_thread_id: Some(thread_id),
                context: context.clone(),
                policy,
            })
            .await
            .unwrap();

        let request = ToolRuntimeRequest {
            thread_id: ThreadId(thread_id),
            ..runtime_request(
                session.session_id,
                ToolAdapterKind::File,
                ToolProviderType::Local,
                "file",
                json!({"path": "docs/SECURITY_EVOLUTION.md"}),
            )
        };

        let events =
            collect_events(kernel.execute_tool_runtime(request.clone()).await.unwrap()).await;

        assert!(
            matches!(&events[0], ToolRuntimeEvent::Request { request } if request.session_id == session.session_id)
        );
        assert!(matches!(&events[1], ToolRuntimeEvent::StreamChunk { .. }));
        assert!(matches!(&events[2], ToolRuntimeEvent::Finalize { result, .. } if result.success));
        assert_eq!(kernel.current_state().await, KernelState::Running);

        let attached = kernel.attach_session(session.session_id).await.unwrap();
        assert_eq!(attached.context, context);
        assert_eq!(attached.attached_thread_id, Some(thread_id));

        let seen = seen_requests.lock().await;
        assert_eq!(seen.len(), 1);
        let security = seen[0].security.as_ref().unwrap();
        assert_eq!(security.policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(security.sandbox_profile, ProcessSandboxProfile::Restricted);
        assert!(security
            .policy_reason
            .contains("session policy requested restricted process sandbox"));
        drop(seen);

        let audits = audit_store(&kernel).list_executions(thread_id).await.unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].session_id, session.session_id);
        assert_eq!(audits[0].policy_action, ExecutionPolicyAction::Allow);
        assert_eq!(audits[0].outcome, "finalized");

        let memory_system = kernel.memory_system().expect("memory system should initialize");
        let mut memory = MemoryDocument::default();
        memory.add_fact(
            state_abstraction::Fact::new(
                "gateway certification request cert-001 completed a restricted file-tool run"
                    .to_string(),
                FactCategory::Context,
                0.98,
                thread_id.to_string(),
            )
            .with_mandatory(true),
        );
        memory_system.save_memory(thread_id, &memory).await.unwrap();

        let stored_memory = memory_system.load_memory(thread_id).await.unwrap();
        assert_eq!(stored_memory.facts.len(), 1);
        assert!(stored_memory.has_any_content());
        assert_eq!(stored_memory.segmented_context.recent.facts.len(), 1);

        let retrieved = memory_system
            .retrieve_relevant_context(thread_id, "which gateway request completed certification")
            .await
            .unwrap();
        assert!(retrieved.recent_facts.iter().any(|fact| fact.content.contains("cert-001")));
        let injected = memory_system.inject_to_prompt_with_query(
            "System prompt",
            &stored_memory,
            "which gateway request completed certification",
        );
        assert!(injected.contains("cert-001"));
    }
}
