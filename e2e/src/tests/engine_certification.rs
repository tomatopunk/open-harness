mod common;

#[test]
fn engine_certification_covers_kernel_state_machine_chain() {
    common::run_workspace_test("agent-kernel", "state_machine_plans_valid_kernel_transition_chain");
}

#[test]
fn engine_certification_covers_session_lineage_and_policy_inheritance() {
    common::run_workspace_test("agent-kernel", "test_session_core_create_attach_fork_close");
}

#[test]
fn engine_certification_covers_streaming_runtime_adapter_bus() {
    common::run_workspace_test(
        "agent-kernel",
        "streaming_runtime_shared_bus_exposes_all_adapter_classes",
    );
}

#[test]
fn engine_certification_covers_security_chain_block_and_audit() {
    common::run_workspace_test(
        "agent-kernel",
        "dangerous_bash_command_is_blocked_before_executor_and_audited",
    );
}

#[test]
fn engine_certification_covers_session_runtime_memory_closure() {
    common::run_workspace_test(
        "agent-kernel",
        "session_runtime_security_and_memory_chain_stays_coherent",
    );
}
