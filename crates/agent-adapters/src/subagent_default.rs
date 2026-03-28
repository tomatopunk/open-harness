//! Subagent execution + merge into parent thread state (concurrent pool + timeouts).

use agent_ports::{
    AgentEvent, ChatMessage, EventSink, PortResult, RunId, SubagentExecuteParams,
    SubagentMergeContext, SubagentPort, SubagentResult, SubtaskPlan, ThreadId, ThreadState,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Debug, Clone, Default)]
pub struct DefaultSubagentAdapter;

#[async_trait]
impl SubagentPort for DefaultSubagentAdapter {
    async fn execute_plan(
        &self,
        run_id: RunId,
        _thread_id: ThreadId,
        plan: &SubtaskPlan,
        _state: &ThreadState,
        params: &SubagentExecuteParams,
        sink: &mut EventSink,
    ) -> PortResult<Vec<SubagentResult>> {
        let max_c = params.max_concurrent.max(1) as usize;
        let sem = Arc::new(Semaphore::new(max_c));
        let mut join_set = JoinSet::new();

        for (idx, t) in plan.tasks.iter().enumerate() {
            let task_id = uuid::Uuid::new_v4();
            sink.push(AgentEvent::SubagentTaskStarted { run_id, task_id, goal: t.goal.clone() });
            let permit = sem
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| agent_ports::PortError::Subagent(e.to_string()))?;
            let goal = t.goal.clone();
            let input = t.input.clone();
            let timeout = params.per_task_timeout;
            join_set.spawn(async move {
                let _p = permit;
                let work = async {
                    SubagentResult {
                        task_id,
                        ok: true,
                        output: json!({
                            "goal": goal,
                            "index": idx,
                            "input": input
                        }),
                    }
                };
                let outcome = match timeout {
                    Some(d) => match tokio::time::timeout(d, work).await {
                        Ok(sr) => Ok((idx, sr)),
                        Err(_) => Err((idx, task_id)),
                    },
                    None => Ok((idx, work.await)),
                };
                outcome
            });
        }

        let mut indexed: Vec<(usize, SubagentResult)> = Vec::new();
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(Ok((idx, sr))) => {
                    sink.push(AgentEvent::SubagentTaskCompleted {
                        run_id,
                        task_id: sr.task_id,
                        ok: sr.ok,
                    });
                    indexed.push((idx, sr));
                }
                Ok(Err((idx, task_id))) => {
                    sink.push(AgentEvent::SubagentTaskTimedOut { run_id, task_id });
                    let sr = SubagentResult {
                        task_id,
                        ok: false,
                        output: json!({ "error": "timeout" }),
                    };
                    sink.push(AgentEvent::SubagentTaskCompleted { run_id, task_id, ok: false });
                    indexed.push((idx, sr));
                }
                Err(e) => return Err(agent_ports::PortError::Subagent(format!("join: {e}"))),
            }
        }

        indexed.sort_by_key(|(i, _)| *i);
        Ok(indexed.into_iter().map(|(_, r)| r).collect())
    }

    async fn merge(
        &self,
        ctx: &SubagentMergeContext,
        results: &[SubagentResult],
    ) -> PortResult<ThreadState> {
        let mut st = ctx.state.clone();
        let arr: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "task_id": r.task_id,
                    "ok": r.ok,
                    "output": r.output
                })
            })
            .collect();
        let policy_version = ctx.state.governance_marks.policy_version.clone().unwrap_or_default();
        let payload = json!({
            "subagent_results": arr,
            "policy_version": policy_version,
        });
        st.messages.push(ChatMessage { role: "assistant".into(), content: payload });
        Ok(st)
    }
}
