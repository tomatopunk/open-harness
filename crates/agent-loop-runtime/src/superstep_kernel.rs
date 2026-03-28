//! P1 **superstep 调度内核**：`prepare_tasks` → `execute_tasks` → `apply_writes`。
//!
//! - **prepare_tasks**：新一轮外层超步开始，清空 [`agent_ports::PregelMeta::staged_tasks`]。
//! - **prepare**：`prepare::*` 将 PULL/PUSH 任务写入 `staged_tasks`（与 LangGraph `prepare_next_tasks` 对齐的 harness 落点）。
//! - **execute**：`execute::*` 承载并发执行面（工具扇出等），与 prepare 阶段写入的 PUSH 信封一一对应。
//! - **apply_writes_after_node**：节点副作用提交后 bump channel + 记录 `pending_write_queue`（[`crate::pregel::bump_after_node`]）。

use crate::pregel;
use crate::scheduler;

/// Phase 1 — 新外层超步：清空本回合待执行任务队列。
#[inline]
pub fn prepare_tasks(state: &mut agent_ports::ThreadState) {
    scheduler::begin_outer_superstep(state);
}

/// 任务准备（PULL 相位节点、PUSH 扇出槽位）。
pub mod prepare {
    pub use crate::scheduler::{prepare_pull_task, prepare_subagent_fanout, prepare_tool_fanout};
}

/// Phase 3 — 节点完成后写入 channel 版本与 pending writes。
#[inline]
pub fn apply_writes_after_node(state: &mut agent_ports::ThreadState, node_id: &str) {
    pregel::bump_after_node(state, node_id);
}

/// Phase 2 — 可并发执行单元（工具扇出等）。
pub mod execute {
    use agent_ports::{RunId, ThreadId, ToolCallSpec};
    use futures::stream::{self, StreamExt};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tracing::warn;

    /// 并行执行工具调用，**按 `calls` 顺序**对齐结果（以 `tool_call_id` 回填，完成顺序任意）。
    pub(crate) async fn invoke_tool_calls_in_call_order(
        tools: Arc<dyn agent_ports::ToolPort>,
        run_id: RunId,
        thread_id: ThreadId,
        calls: &[ToolCallSpec],
        max_concurrent: usize,
    ) -> Vec<Result<Value, String>> {
        let pairs: Vec<(String, Result<Value, String>)> =
            stream::iter(calls.iter().cloned().map(|call| {
                let tools = tools.clone();
                async move {
                    let id = call.call_id.clone();
                    let res =
                        tools.invoke(run_id, thread_id, &call).await.map_err(|e| e.to_string());
                    (id, res)
                }
            }))
            .buffer_unordered(max_concurrent.max(1))
            .collect()
            .await;

        let mut map: HashMap<String, Result<Value, String>> = HashMap::with_capacity(pairs.len());
        for (id, res) in pairs {
            if map.insert(id.clone(), res).is_some() {
                warn!("duplicate tool_call_id in concurrent tool results: {}", id);
            }
        }

        calls
            .iter()
            .map(|c| {
                map.remove(&c.call_id).unwrap_or_else(|| {
                    Err(format!("missing tool result for call_id={} name={}", c.call_id, c.name))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::execute::invoke_tool_calls_in_call_order;
    use agent_ports::{ThreadId, ToolCallSpec, ToolManifest};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::Instant;

    /// 完成顺序与 `calls` 不同；结果向量必须与 `calls` 顺序一致。
    struct DelayByCallIdTool {
        completion_log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl agent_ports::ToolPort for DelayByCallIdTool {
        fn manifests(&self) -> Vec<ToolManifest> {
            vec![ToolManifest {
                name: "echo".into(),
                description: None,
                input_schema: None,
                capability_tags: Vec::new(),
                risk_level: agent_ports::RiskLevel::Low,
                timeout_ms: 5000,
                retry_max: 0,
                side_effect_class: agent_ports::SideEffectClass::None,
            }]
        }

        async fn invoke(
            &self,
            _run_id: agent_ports::RunId,
            _thread_id: ThreadId,
            call: &ToolCallSpec,
        ) -> agent_ports::PortResult<Value> {
            let ms = match call.call_id.as_str() {
                "first" => 30u64,
                "second" => 5,
                "third" => 15,
                _ => 0,
            };
            tokio::time::sleep(Duration::from_millis(ms)).await;
            self.completion_log
                .lock()
                .expect("completion_log mutex poisoned in test")
                .push(call.call_id.clone());
            Ok(json!({ "call_id": call.call_id }))
        }
    }

    #[tokio::test]
    async fn tool_results_follow_calls_order_not_completion_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let tools: Arc<dyn agent_ports::ToolPort> =
            Arc::new(DelayByCallIdTool { completion_log: Arc::clone(&log) });
        let calls = vec![
            ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "first".into() },
            ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "second".into() },
            ToolCallSpec { name: "echo".into(), args: json!({}), call_id: "third".into() },
        ];
        let started = Instant::now();
        let out = invoke_tool_calls_in_call_order(
            tools,
            agent_ports::RunId::default(),
            ThreadId::new_v4(),
            &calls,
            8,
        )
        .await;
        let elapsed = started.elapsed();
        assert!(elapsed.as_millis() >= 25, "parallel fan-out should overlap sleeps");

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].as_ref().expect("first tool result"), &json!({"call_id": "first"}));
        assert_eq!(out[1].as_ref().expect("second tool result"), &json!({"call_id": "second"}));
        assert_eq!(out[2].as_ref().expect("third tool result"), &json!({"call_id": "third"}));

        let done = log.lock().expect("completion log mutex poisoned in test").clone();
        assert_ne!(
            done,
            vec!["first", "second", "third"],
            "completion order should differ from call order for this fixture"
        );
    }
}
