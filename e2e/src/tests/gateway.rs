mod common;

#[test]
fn lifecycle_and_plugin_failure_regressions_are_bound_to_acceptance() {
    common::run_workspace_test(
        "agent-kernel",
        "test_initialize_runs_refactor_lifecycle_stages_in_order",
    );
    common::run_workspace_test(
        "agent-kernel",
        "test_kernel_initialization_reports_plugin_failures_with_context",
    );
}
