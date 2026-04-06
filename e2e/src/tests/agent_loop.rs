mod common;

#[test]
fn config_precedence_and_missing_backend_regressions_are_bound_to_acceptance() {
    common::run_workspace_test(
        "unified-config",
        "test_load_unified_config_applies_skill_precedence",
    );
    common::run_workspace_test(
        "agent-kernel",
        "test_initialize_reports_missing_memory_backend_with_context",
    );
}
