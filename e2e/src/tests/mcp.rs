mod common;

#[test]
fn mcp_reconnect_regression_is_bound_to_acceptance() {
    common::run_workspace_test("mcp-bridge", "reconnect_does_not_reuse_stale_tool_cache");
}
