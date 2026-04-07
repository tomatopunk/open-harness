use agent_kernel::{
    AgentKernel, AgentLoopConfig, AgentLoopState, HookPhase, HookSystem, KernelConfig,
};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;
use uuid::Uuid;

#[tokio::test]
async fn test_complete_agent_loop_hook_chain() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.plugins_dir = temp_dir.path().join("plugins");
    kernel_config.agent_loop = AgentLoopConfig {
        enabled: true,
        max_iterations: 2,
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
                Box::new(move |state| {
                    let hook_phases = hook_phases.clone();
                    let iteration = state.iteration;
                    let active = state.active;
                    Box::pin(async move {
                        let mut phases = hook_phases.lock().await;
                        phases.push((phase, iteration, active));
                        Ok(())
                    })
                }),
            )
            .await;
    }

    kernel.initialize().await.unwrap();
    kernel
        .agent_loop()
        .unwrap()
        .start_loop(Uuid::new_v4(), "collect hooks".to_string())
        .await
        .unwrap();

    let phases = hook_phases.lock().await.clone();
    let before_iteration_count =
        phases.iter().filter(|(phase, _, _)| *phase == HookPhase::BeforeIteration).count();
    let after_iteration_count =
        phases.iter().filter(|(phase, _, _)| *phase == HookPhase::AfterIteration).count();
    let before_completion_count =
        phases.iter().filter(|(phase, _, _)| *phase == HookPhase::BeforeCompletion).count();

    assert_eq!(hooks.hook_count().await, 5);
    assert_eq!(phases.first(), Some(&(HookPhase::BeforeLoop, 0, true)));
    assert_eq!(phases.last(), Some(&(HookPhase::AfterCompletion, 2, false)));
    assert_eq!(before_iteration_count, 3);
    assert_eq!(after_iteration_count, 2);
    assert_eq!(before_completion_count, 2);
}

#[tokio::test]
async fn test_agent_loop_state_transitions_via_hooks() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.plugins_dir = temp_dir.path().join("plugins");
    kernel_config.agent_loop = AgentLoopConfig {
        enabled: false,
        max_iterations: 5,
        completion_promise: "DONE".to_string(),
        debounce_seconds: 0,
    };

    let kernel = AgentKernel::new(kernel_config.clone());

    assert!(kernel.agent_loop().is_none());

    kernel.initialize().await.unwrap();

    assert!(kernel.agent_loop().is_none());
    assert!(kernel.llm_provider().is_some());
    assert!(kernel.memory_system().is_some());
    assert!(kernel.channel_manager().is_some());
}

#[tokio::test]
async fn test_kernel_lifecycle_integration_with_agent_loop() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.plugins_dir = temp_dir.path().join("plugins");

    let kernel = AgentKernel::new(kernel_config.clone());

    assert!(kernel.llm_provider().is_none());
    assert!(kernel.mcp_bridge().is_none());
    assert!(kernel.agent_loop().is_none());
    assert!(kernel.memory_system().is_none());

    kernel.initialize().await.unwrap();

    assert!(kernel.llm_provider().is_some());
    assert!(kernel.mcp_bridge().is_some());
    assert!(kernel.agent_loop().is_some());
    assert!(kernel.memory_system().is_some());
}

#[tokio::test]
async fn test_hook_system_with_multiple_kernels() {
    let temp_dir = tempdir().unwrap();

    let mut kernel_config = KernelConfig::default();
    kernel_config.workspace_root = temp_dir.path().to_path_buf();
    kernel_config.plugins_dir = temp_dir.path().join("plugins");

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
    kernel_config.plugins_dir = temp_dir.path().join("plugins");

    let _kernel = AgentKernel::new(kernel_config.clone());
    let hooks = HookSystem::new();

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

    assert_eq!(config.agent_loop.max_iterations, 200);
    assert_eq!(config.agent_loop.completion_promise, "END");
    assert_eq!(config.agent_loop.debounce_seconds, 1);
}
