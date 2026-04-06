//! Test Data - 测试数据预设
//!
//! 提供各种测试用的预置数据

use serde_json::Value;

/// 创建标准用户消息
pub fn standard_user_message() -> Value {
    serde_json::json!({
        "role": "user",
        "content": "hello from e2e test"
    })
}

/// 创建多轮对话消息
pub fn multi_turn_messages() -> Vec<Value> {
    vec![
        serde_json::json!({
            "role": "system",
            "content": "You are a helpful assistant."
        }),
        serde_json::json!({
            "role": "user",
            "content": "What is the capital of France?"
        }),
    ]
}

/// 钉钉 Webhook 测试数据
pub mod dingtalk {
    pub fn simple_text() -> (String, String) {
        ("hello from e2e test".to_string(), "test-event-001".to_string())
    }
}

/// 企业微信 Webhook 测试数据
pub mod wecom {
    pub fn simple_text() -> (String, String) {
        ("hello from e2e test".to_string(), "test-user-001".to_string())
    }
}

/// Agent 创建测试数据
pub mod agent {
    pub fn test_agent() -> (&'static str, &'static str, Option<&'static str>) {
        ("E2E Test Agent", "gpt-4", Some("You are a test assistant."))
    }
}
