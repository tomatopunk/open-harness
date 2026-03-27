use protocol_compat::Configurable;
use runtime_kernel::RuntimeKernel;

#[tokio::test]
async fn regression_guardrails_block_search_without_sandbox() {
    let kernel = RuntimeKernel::default();
    let ctx = kernel
        .prepare_with_input(
            Configurable { sandbox_enabled: Some(false), ..Configurable::default() },
            vec![serde_json::json!("please search rust docs and read file")],
        )
        .await
        .unwrap_or_else(|err| panic!("prepare_with_input failed: {err}"));

    assert!(ctx.blocked_tools.iter().any(|t| t == "web_search"));
    assert!(ctx.tool_calls.iter().any(|t| t.tool_name == "read_file"));
}
