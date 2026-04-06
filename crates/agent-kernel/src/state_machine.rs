use crate::lifecycle::{shutdown_stages, LifecycleStage};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelState {
    Created,
    Initialized,
    Running,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelEvent {
    Initialize,
    Start,
    Stop,
    ToolExecutionStarted {
        session_id: Uuid,
        tool_call_id: String,
        tool_name: String,
    },
    ToolExecutionFinished {
        session_id: Uuid,
        tool_call_id: String,
        tool_name: String,
        success: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelGuard {
    StateIs(KernelState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelSideEffect {
    InitializeProviderAdapter,
    InitializeMcpAdapter,
    InitializePluginAdapter,
    InitializeLoopStateAdapter,
    InitializeChannelAdapter,
    RunLifecycleStages(Vec<LifecycleStage>),
    LoadPlugins,
    InitializePlugins,
    StartPlugins,
    StartChannels,
    PublishKernelStarted,
    PublishKernelStopped,
    StopChannels,
    StopPlugins,
    UnloadPlugins,
    RecordToolExecutionStart {
        session_id: Uuid,
        tool_call_id: String,
        tool_name: String,
    },
    RecordToolExecutionFinish {
        session_id: Uuid,
        tool_call_id: String,
        tool_name: String,
        success: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelTransition {
    pub from: KernelState,
    pub event: KernelEvent,
    pub to: KernelState,
    pub guard: KernelGuard,
    pub side_effects: Vec<KernelSideEffect>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("illegal kernel transition from {from:?} via {event:?}; required guard: {guard:?}")]
pub struct KernelStateTransitionError {
    pub from: KernelState,
    pub event: KernelEvent,
    pub guard: KernelGuard,
}

pub struct KernelStateMachine;

impl KernelStateMachine {
    pub fn transition(
        from: KernelState,
        event: KernelEvent,
    ) -> Result<KernelTransition, KernelStateTransitionError> {
        match (from, &event) {
            (KernelState::Created, KernelEvent::Initialize) => Ok(KernelTransition {
                from,
                event,
                to: KernelState::Initialized,
                guard: KernelGuard::StateIs(KernelState::Created),
                side_effects: vec![
                    KernelSideEffect::InitializeProviderAdapter,
                    KernelSideEffect::InitializeMcpAdapter,
                    KernelSideEffect::InitializePluginAdapter,
                    KernelSideEffect::InitializeLoopStateAdapter,
                    KernelSideEffect::InitializeChannelAdapter,
                    KernelSideEffect::RunLifecycleStages(initialize_stages()),
                ],
            }),
            (KernelState::Initialized, KernelEvent::Start) => Ok(KernelTransition {
                from,
                event,
                to: KernelState::Running,
                guard: KernelGuard::StateIs(KernelState::Initialized),
                side_effects: vec![
                    KernelSideEffect::RunLifecycleStages(start_stages()),
                    KernelSideEffect::LoadPlugins,
                    KernelSideEffect::InitializePlugins,
                    KernelSideEffect::StartPlugins,
                    KernelSideEffect::StartChannels,
                    KernelSideEffect::PublishKernelStarted,
                ],
            }),
            (KernelState::Running, KernelEvent::Stop) => Ok(KernelTransition {
                from,
                event,
                to: KernelState::Stopped,
                guard: KernelGuard::StateIs(KernelState::Running),
                side_effects: vec![
                    KernelSideEffect::PublishKernelStopped,
                    KernelSideEffect::StopChannels,
                    KernelSideEffect::StopPlugins,
                    KernelSideEffect::UnloadPlugins,
                    KernelSideEffect::RunLifecycleStages(shutdown_stages()),
                ],
            }),
            (KernelState::Running, KernelEvent::ToolExecutionStarted { .. }) => {
                Ok(KernelTransition {
                    from,
                    event: event.clone(),
                    to: KernelState::Running,
                    guard: KernelGuard::StateIs(KernelState::Running),
                    side_effects: vec![tool_execution_started_effect(&event)],
                })
            }
            (KernelState::Running, KernelEvent::ToolExecutionFinished { .. }) => {
                Ok(KernelTransition {
                    from,
                    event: event.clone(),
                    to: KernelState::Running,
                    guard: KernelGuard::StateIs(KernelState::Running),
                    side_effects: vec![tool_execution_finished_effect(&event)],
                })
            }
            _ => Err(KernelStateTransitionError { from, guard: required_guard(&event), event }),
        }
    }
}

fn initialize_stages() -> Vec<LifecycleStage> {
    vec![
        LifecycleStage::BeforeInit,
        LifecycleStage::InitConfig,
        LifecycleStage::InitProvider,
        LifecycleStage::InitMcp,
        LifecycleStage::InitPlugin,
        LifecycleStage::InitLoopState,
        LifecycleStage::Init,
        LifecycleStage::AfterInit,
    ]
}

fn start_stages() -> Vec<LifecycleStage> {
    vec![LifecycleStage::BeforeStart, LifecycleStage::Start, LifecycleStage::AfterStart]
}

fn required_guard(event: &KernelEvent) -> KernelGuard {
    match event {
        KernelEvent::Initialize => KernelGuard::StateIs(KernelState::Created),
        KernelEvent::Start => KernelGuard::StateIs(KernelState::Initialized),
        KernelEvent::Stop
        | KernelEvent::ToolExecutionStarted { .. }
        | KernelEvent::ToolExecutionFinished { .. } => KernelGuard::StateIs(KernelState::Running),
    }
}

fn tool_execution_started_effect(event: &KernelEvent) -> KernelSideEffect {
    match event {
        KernelEvent::ToolExecutionStarted { session_id, tool_call_id, tool_name } => {
            KernelSideEffect::RecordToolExecutionStart {
                session_id: *session_id,
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
            }
        }
        _ => unreachable!("tool execution start effect requires ToolExecutionStarted event"),
    }
}

fn tool_execution_finished_effect(event: &KernelEvent) -> KernelSideEffect {
    match event {
        KernelEvent::ToolExecutionFinished { session_id, tool_call_id, tool_name, success } => {
            KernelSideEffect::RecordToolExecutionFinish {
                session_id: *session_id,
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                success: *success,
            }
        }
        _ => unreachable!("tool execution finish effect requires ToolExecutionFinished event"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_plans_valid_kernel_transition_chain() {
        let session_id = Uuid::nil();
        let mut state = KernelState::Created;
        let transitions = [
            KernelEvent::Initialize,
            KernelEvent::Start,
            KernelEvent::ToolExecutionStarted {
                session_id,
                tool_call_id: "tool-call-1".to_string(),
                tool_name: "search".to_string(),
            },
            KernelEvent::ToolExecutionFinished {
                session_id,
                tool_call_id: "tool-call-1".to_string(),
                tool_name: "search".to_string(),
                success: true,
            },
            KernelEvent::Stop,
        ]
        .into_iter()
        .map(|event| {
            let transition = KernelStateMachine::transition(state, event).unwrap();
            state = transition.to;
            transition
        })
        .collect::<Vec<_>>();

        assert_eq!(state, KernelState::Stopped);
        assert_eq!(transitions[0].from, KernelState::Created);
        assert_eq!(transitions[0].to, KernelState::Initialized);
        assert_eq!(transitions[1].from, KernelState::Initialized);
        assert_eq!(transitions[1].to, KernelState::Running);
        assert_eq!(transitions[2].to, KernelState::Running);
        assert_eq!(transitions[3].to, KernelState::Running);
        assert_eq!(transitions[4].from, KernelState::Running);
        assert_eq!(transitions[4].to, KernelState::Stopped);
    }

    #[test]
    fn state_machine_rejects_invalid_transition_with_typed_error() {
        let error = KernelStateMachine::transition(KernelState::Created, KernelEvent::Start)
            .expect_err("start should be rejected before initialize");

        assert_eq!(
            error,
            KernelStateTransitionError {
                from: KernelState::Created,
                event: KernelEvent::Start,
                guard: KernelGuard::StateIs(KernelState::Initialized),
            }
        );
    }

    #[test]
    fn state_machine_matches_kernel_orchestration_side_effects() {
        let initialize =
            KernelStateMachine::transition(KernelState::Created, KernelEvent::Initialize).unwrap();
        assert_eq!(
            initialize.side_effects,
            vec![
                KernelSideEffect::InitializeProviderAdapter,
                KernelSideEffect::InitializeMcpAdapter,
                KernelSideEffect::InitializePluginAdapter,
                KernelSideEffect::InitializeLoopStateAdapter,
                KernelSideEffect::InitializeChannelAdapter,
                KernelSideEffect::RunLifecycleStages(vec![
                    LifecycleStage::BeforeInit,
                    LifecycleStage::InitConfig,
                    LifecycleStage::InitProvider,
                    LifecycleStage::InitMcp,
                    LifecycleStage::InitPlugin,
                    LifecycleStage::InitLoopState,
                    LifecycleStage::Init,
                    LifecycleStage::AfterInit,
                ]),
            ]
        );

        let start =
            KernelStateMachine::transition(KernelState::Initialized, KernelEvent::Start).unwrap();
        assert_eq!(
            start.side_effects,
            vec![
                KernelSideEffect::RunLifecycleStages(vec![
                    LifecycleStage::BeforeStart,
                    LifecycleStage::Start,
                    LifecycleStage::AfterStart,
                ]),
                KernelSideEffect::LoadPlugins,
                KernelSideEffect::InitializePlugins,
                KernelSideEffect::StartPlugins,
                KernelSideEffect::StartChannels,
                KernelSideEffect::PublishKernelStarted,
            ]
        );

        let stop = KernelStateMachine::transition(KernelState::Running, KernelEvent::Stop).unwrap();
        assert_eq!(
            stop.side_effects,
            vec![
                KernelSideEffect::PublishKernelStopped,
                KernelSideEffect::StopChannels,
                KernelSideEffect::StopPlugins,
                KernelSideEffect::UnloadPlugins,
                KernelSideEffect::RunLifecycleStages(shutdown_stages()),
            ]
        );

        let tool_start = KernelStateMachine::transition(
            KernelState::Running,
            KernelEvent::ToolExecutionStarted {
                session_id: Uuid::nil(),
                tool_call_id: "tool-call-2".to_string(),
                tool_name: "search".to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            tool_start.side_effects,
            vec![KernelSideEffect::RecordToolExecutionStart {
                session_id: Uuid::nil(),
                tool_call_id: "tool-call-2".to_string(),
                tool_name: "search".to_string(),
            }]
        );

        let tool_finish = KernelStateMachine::transition(
            KernelState::Running,
            KernelEvent::ToolExecutionFinished {
                session_id: Uuid::nil(),
                tool_call_id: "tool-call-2".to_string(),
                tool_name: "search".to_string(),
                success: false,
            },
        )
        .unwrap();
        assert_eq!(
            tool_finish.side_effects,
            vec![KernelSideEffect::RecordToolExecutionFinish {
                session_id: Uuid::nil(),
                tool_call_id: "tool-call-2".to_string(),
                tool_name: "search".to_string(),
                success: false,
            }]
        );
    }
}
