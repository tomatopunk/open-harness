//! Subagent execution + merge into parent thread state.

use agent_ports::{
    ChatMessage, PortResult, RunId, SubagentMergeContext, SubagentPort, SubagentResult,
    SubtaskPlan, ThreadId, ThreadState,
};
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct DefaultSubagentAdapter {
    pub max_concurrent: usize,
}

impl Default for DefaultSubagentAdapter {
    fn default() -> Self {
        Self { max_concurrent: 4 }
    }
}

#[async_trait]
impl SubagentPort for DefaultSubagentAdapter {
    async fn execute_plan(
        &self,
        _run_id: RunId,
        _thread_id: ThreadId,
        plan: &SubtaskPlan,
        _state: &ThreadState,
    ) -> PortResult<Vec<SubagentResult>> {
        let mut out = Vec::new();
        for (i, t) in plan.tasks.iter().enumerate().take(self.max_concurrent) {
            out.push(SubagentResult {
                task_id: uuid::Uuid::new_v4(),
                ok: true,
                output: json!({
                    "goal": t.goal,
                    "index": i,
                    "input": t.input
                }),
            });
        }
        Ok(out)
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
        let payload = json!({ "subagent_results": arr });
        st.messages.push(ChatMessage { role: "assistant".into(), content: payload });
        Ok(st)
    }
}
