mod common;

#[test]
fn capability_matrix_covers_session_and_state_machine_paths() {
    common::run_workspace_test("agent-kernel", "test_session_core_create_attach_fork_close");
    common::run_workspace_test(
        "agent-kernel",
        "test_session_core_rejects_invalid_parent_reference",
    );
    common::run_workspace_test("agent-kernel", "state_machine_plans_valid_kernel_transition_chain");
}

#[test]
fn capability_matrix_covers_streaming_file_agent_fork_mcp_skill_and_web_paths() {
    common::run_workspace_test(
        "agent-kernel",
        "streaming_runtime_shared_bus_exposes_all_adapter_classes",
    );
}

#[test]
fn capability_matrix_covers_file_security_injection_and_memory_closure() {
    common::run_workspace_test(
        "agent-kernel",
        "runtime_execution_path_always_injects_security_context_before_adapter_invocation",
    );
    common::run_workspace_test(
        "agent-kernel",
        "session_runtime_security_and_memory_chain_stays_coherent",
    );
}

#[test]
fn capability_matrix_covers_bash_security_allow_and_block_paths() {
    common::run_workspace_test("agent-kernel", "harmless_bash_command_is_allowed_and_audited");
    common::run_workspace_test(
        "agent-kernel",
        "dangerous_bash_command_is_blocked_before_executor_and_audited",
    );
}

#[test]
fn capability_matrix_covers_mcp_bridge_reconnect_regression() {
    common::run_workspace_test("mcp-bridge", "reconnect_does_not_reuse_stale_tool_cache");
}
