//! Open Harness Kernel - 主入口点
//!
//! 启动 agent kernel 并加载插件

use agent_kernel::{AgentKernel, KernelConfig};
use std::path::PathBuf;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=========================================");
    info!("  Open Harness - Agent Kernel");
    info!("=========================================");

    // 加载配置
    let config_path = PathBuf::from("config.yaml");
    let kernel_config = KernelConfig::from_file(&config_path)?;

    info!("Workspace root: {:?}", kernel_config.workspace_root);
    info!("Plugins directory: {:?}", kernel_config.plugins_dir);
    info!("Dev mode: {}", kernel_config.dev_mode);

    // 创建 kernel
    let kernel = AgentKernel::new(kernel_config.clone());
    let kernel_for_signal = kernel.clone();

    // 设置信号处理
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("\nReceived shutdown signal, stopping kernel...");
            if let Err(e) = kernel_for_signal.stop().await {
                error!("Error stopping kernel: {}", e);
            }
        }
    });

    // 初始化并启动 kernel
    info!("Initializing kernel...");
    kernel.initialize().await?;

    info!("Starting kernel...");
    kernel.start().await?;

    info!("Kernel is running. Press Ctrl+C to stop.");

    // 等待
    tokio::signal::ctrl_c().await?;

    Ok(())
}
