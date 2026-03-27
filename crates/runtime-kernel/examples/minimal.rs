use protocol_compat::Configurable;
use runtime_kernel::RuntimeKernel;

#[tokio::main]
async fn main() {
    let kernel = RuntimeKernel::default();
    let ctx = kernel
        .prepare_with_input(
            Configurable::default(),
            vec![serde_json::json!("fact: likes rust"), serde_json::json!("please search docs")],
        )
        .await
        .expect("kernel prepare");
    let events = kernel.render_events(&ctx);
    println!("events={}", events.len());
}
