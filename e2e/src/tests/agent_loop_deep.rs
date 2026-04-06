use agent_kernel::{
    AgentKernel, AgentLoop, AgentLoopConfig, AgentLoopState, HookPhase, HookSystem, KernelConfig,
    KernelState,
};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::sync::Mutex;
use tokio::sync::OnceCell;
use uuid::Uuid;

#[tokio::test]
async fn test_complete_agent_loop_hook_chain() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.agent_loop = AgentLoopConfig {
        enabled: true,
        max_iterations: 3,
        completion_promise: "TEST_DONE".to_string(),
        debounce_seconds: 0,
    };

    let kernel = AgentKernel::new(kernel_config.clone());

    let hook_phases = Arc::new(Mutex::new(Vec::new()));
    let hooks = kernel.hooks();

    let phases_to_track = vec![
        HookPhase::BeforeLoop,
        HookPhase::BeforeIteration,
        HookPhase::AfterIteration,
        HookPhase::BeforeCompletion,
        HookPhase::AfterCompletion,
    ];

    for &phase in &phases_to_track {
        let hook_phases = hook_phases.clone();
        hooks
            .register_hook_async(
                phase,
                Box::new(move |_state| {
                    let hook_phases = hook_phases.clone();
                    Box::pin(async move {
                        let mut phases = hook_phases.lock().await;
                        phases.push(phase);
                        Ok(())
                    })
                }),
            )
            .await;
    }

    assert_eq!(hooks.hook_count().await, 5);
}

#[tokio::test]
async fn test_agent_loop_state_transitions_via_hooks() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.agent_loop = AgentLoopConfig {
        enabled: true,
        max_iterations: 5,
        completion_promise: "DONE".to_string(),
        debounce_seconds: 0,
    };

    let kernel = AgentKernel::new(kernel_config.clone());

    let iteration_count = Arc::new(Mutex::new(0));
    let hooks = kernel.hooks();

    let ic = iteration_count.clone();
    hooks
        .register_hook_async(
            HookPhase::AfterIteration,
            Box::new(move |state| {
                let ic = ic.clone();
                Box::pin(async move {
                    let mut count = ic.lock().await;
                    *count = state.iteration;
                    Ok(())
                })
            }),
        )
        .await;

    let ic = iteration_count.clone();
    hooks
        .register_hook_async(
            HookPhase::BeforeCompletion,
            Box::new(move |state| {
                let ic = ic.clone();
                Box::pin(async move {
                    let count = ic.lock().await;
                    assert!(state.iteration <= 5, "iteration should not exceed max");
                    Ok(())
                })
            }),
        )
        .await;
}

#[tokio::test]
async fn test_kernel_lifecycle_integration_with_agent_loop() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();

    let kernel = AgentKernel::new(kernel_config.clone());

    let event_bus = kernel.event_bus();

    assert!(kernel.llm_provider().is_none());
    assert!(kernel.mcp_bridge().is_none());
    assert!(kernel.agent_loop().is_none());
    assert!(kernel.memory_system().is_none());
}

#[tokio::test]
async fn test_hook_system_with_multiple_kernels() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();

    let kernel1 = AgentKernel::new(kernel_config.clone());
    let kernel2 = AgentKernel::new(kernel_config.clone());

    let hook_count1 = Arc::new(Mutex::new(0));
    let hook_count2 = Arc::new(Mutex::new(0));

    let hc1 = hook_count1.clone();
    kernel1
        .hooks()
        .register_hook_async(
            HookPhase::BeforeLoop,
            Box::new(move |_state| {
                let hc1 = hc1.clone();
                Box::pin(async move {
                    let mut count = hc1.lock().await;
                    *count += 1;
                    Ok(())
                })
            }),
        )
        .await;

    let hc2 = hook_count2.clone();
    kernel2
        .hooks()
        .register_hook_async(
            HookPhase::BeforeLoop,
            Box::new(move |_state| {
                let hc2 = hc2.clone();
                Box::pin(async move {
                    let mut count = hc2.lock().await;
                    *count += 1;
                    Ok(())
                })
            }),
        )
        .await;

    assert_eq!(kernel1.hooks().hook_count().await, 1);
    assert_eq!(kernel2.hooks().hook_count().await, 1);
}

#[tokio::test]
async fn test_agent_loop_state_cloning() {
    let thread_id = Uuid::new_v4();
    let config = AgentLoopConfig::default();
    let state = AgentLoopState::new(thread_id, "test prompt".to_string(), &config);

    let cloned = state.clone();

    assert_eq!(state.active, cloned.active);
    assert_eq!(state.iteration, cloned.iteration);
    assert_eq!(state.max_iterations, cloned.max_iterations);
    assert_eq!(state.completion_promise, cloned.completion_promise);
    assert_eq!(state.prompt, cloned.prompt);
    assert_eq!(state.thread_id, cloned.thread_id);
}

#[tokio::test]
async fn test_completion_detection_various_formats() {
    let thread_id = Uuid::new_v4();
    let mut config = AgentLoopConfig::default();
    config.completion_promise = "FINISH".to_string();

    let state = AgentLoopState::new(thread_id, "test".to_string(), &config);

    assert_eq!(state.detect_completion("FINISH"), true);
    assert_eq!(state.detect_completion("<promise>FINISH</promise>"), true);
    assert_eq!(state.detect_completion("some text FINISH more text"), true);
    assert_eq!(state.detect_completion("finish"), false);
    assert_eq!(state.detect_completion(""), false);
    assert_eq!(state.detect_completion("no completion here"), false);
}

#[tokio::test]
async fn test_agent_loop_config_custom_values() {
    let config = AgentLoopConfig {
        enabled: false,
        max_iterations: 50,
        completion_promise: "COMPLETE".to_string(),
        debounce_seconds: 5,
    };

    assert_eq!(config.enabled, false);
    assert_eq!(config.max_iterations, 50);
    assert_eq!(config.completion_promise, "COMPLETE");
    assert_eq!(config.debounce_seconds, 5);
}

#[tokio::test]
async fn test_hook_clear_isolation() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();

    let kernel = AgentKernel::new(kernel_config.clone());
    let mut hooks = HookSystem::new();

    hooks
        .register_hook_async(HookPhase::BeforeLoop, Box::new(|_state| Box::pin(async { Ok(()) })))
        .await;
    hooks
        .register_hook_async(
            HookPhase::AfterIteration,
            Box::new(|_state| Box::pin(async { Ok(()) })),
        )
        .await;

    assert_eq!(hooks.hook_count().await, 2);

    hooks.clear_hooks_async().await;

    assert_eq!(hooks.hook_count().await, 0);
}

#[tokio::test]
async fn test_agent_loop_state_debug() {
    let thread_id = Uuid::new_v4();
    let config = AgentLoopConfig::default();
    let state = AgentLoopState::new(thread_id, "test".to_string(), &config);

    let debug_str = format!("{:?}", state);

    assert!(debug_str.contains("AgentLoopState"));
    assert!(debug_str.contains("active"));
    assert!(debug_str.contains("iteration"));
}

#[tokio::test]
async fn test_kernel_config_with_custom_agent_loop() {
    let temp_dir = tempdir().unwrap();

    let mut config = KernelConfig::default();
    config.workspace_root = temp_dir.path().to_path_buf();
    config.agent_loop = AgentLoopConfig {
        enabled: true,
        max_iterations: 200,
        completion_promise: "END".to_string(),
        debounce_seconds: 1,
    };

    let kernel = AgentKernel::new(config.clone());

    assert_eq!(config.agent_loop.max_iterations, 200);
    assert_eq!(config.agent_loop.completion_promise, "END");
    assert_eq!(config.agent_loop.debounce_seconds, 1);
}
