//! Server Management - 测试服务器管理
//!
//! 用于检查服务器是否就绪，等待启动完成

use anyhow::Result;
use std::time::Duration;
use tracing::info;

use crate::helpers::api_client::OpenHarnessClient;

/// 检查服务器是否就绪
pub async fn wait_for_server_ready(
    client: &OpenHarnessClient,
    max_retries: u32,
    interval_ms: u64,
) -> Result<bool> {
    info!("Waiting for Open Harness servers to become ready...");

    for attempt in 1..=max_retries {
        if let Ok(gateway_resp) = client.gateway_health_check().await {
            if gateway_resp.status().is_success() {
                if let Ok(manage_resp) = client.manage_health_check().await {
                    if manage_resp.status().is_success() {
                        if let Ok(channels_resp) = client.channels_health_check().await {
                            if channels_resp.status().is_success() {
                                info!("All servers are ready after {} attempts", attempt);
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }

        info!(
            "Attempt {} of {} failed, waiting {}ms...",
            attempt, max_retries, interval_ms
        );
        tokio::time::sleep(Duration::from_millis(interval_ms)).await;
    }

    info!("Failed to connect to servers after {} attempts", max_retries);
    Ok(false)
}

/// 健康检查所有端点
pub async fn health_check_all(client: &OpenHarnessClient) -> Result<(bool, bool, bool)> {
    let gateway_ok = match client.gateway_health_check().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    };

    let manage_ok = match client.manage_health_check().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    };

    let channels_ok = match client.channels_health_check().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    };

    Ok((gateway_ok, manage_ok, channels_ok))
}
