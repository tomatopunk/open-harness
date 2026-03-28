//! Pregel-style superstep orchestration over the agent loop（V2：`superstep_kernel` 三阶段管线）。

use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
use crate::budget::RunBudget;
use crate::dispatch::TurnDispatch;
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::lead_outer_superstep::execute_inner_superstep_turn;
use crate::middleware::TurnContext;
use crate::run_config::AgentLoopRunConfig;
use crate::state_reducer::{append_user_messages, apply_run_config_bootstrap};
use agent_ports::{AgentEvent, EventSink, LoopStage, ThreadId, ThreadState};
use graph_runtime_core::GraphRuntime;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

/// After user input while a durable interrupt is pending, record resume cursor and clear
/// interrupt + clarification wait (unified clarify / interrupt resume path).
/// Clears a durable interrupt when new user messages arrive (host-driven resume).
pub fn maybe_resume_from_interrupt(state: &mut ThreadState, user_messages: &[Value]) {
    if user_messages.is_empty() {
        return;
    }
    if let Some(snap) = state.pregel.interrupt.take() {
        state.pregel.last_resume_at = Some(snap.resume_cursor);
        state.clarification_state.pending = false;
        state.clarification_state.prompt = None;
    }
}

/// Run the main loop until finish or max turns (V2 Pregel-scheduled engine).
pub async fn run_agent_loop(
    graph: Arc<GraphRuntime>,
    deps: Arc<AgentLoopDeps>,
    thread_id: ThreadId,
    mut state: ThreadState,
    user_messages: Vec<Value>,
    budget: RunBudget,
    tool_cfg: ToolLoopConfig,
    run_cfg: AgentLoopRunConfig,
) -> AgentLoopResult<(ThreadState, EventSink)> {
    let mut sink = EventSink::default();
    let run = graph
        .start_run(thread_id, state.clone())
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    let run_id = run.run_id;
    sink.push(AgentEvent::RunStarted { thread_id, run_id });

    append_user_messages(&mut state, &user_messages);
    apply_run_config_bootstrap(&mut state, &run_cfg);
    state.migrate_to_latest_schema();
    maybe_resume_from_interrupt(&mut state, &user_messages);

    let turn_ctx_base = TurnContext { thread_id, run_id, run_cfg: run_cfg.clone(), budget };

    let mut turns: u32 = 0;
    let mut last_tool_call_fingerprint: Option<u64> = None;
    loop {
        if turns >= budget.max_turns {
            return Err(AgentLoopError::MaxTurnsExceeded);
        }
        turns += 1;

        let step_seq = state.step_seq;

        debug!(turn = turns, step_seq = ?step_seq, "inner superstep turn");

        match execute_inner_superstep_turn(
            graph.as_ref(),
            deps.as_ref(),
            turn_ctx_base.clone(),
            thread_id,
            run_id,
            step_seq,
            &mut state,
            &mut sink,
            &tool_cfg,
            &run_cfg,
            &mut last_tool_call_fingerprint,
        )
        .await?
        {
            TurnDispatch::Stop => break,
            TurnDispatch::Again => continue,
        }
    }

    let finalize_seq = state.step_seq;
    crate::loop_common::emit_stage(&mut sink, run_id, finalize_seq, LoopStage::Finalize, true);
    let (_handle, state) = graph
        .complete_run(thread_id, run_id, state, "completed")
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    crate::loop_common::emit_stage(&mut sink, run_id, state.step_seq, LoopStage::Finalize, false);
    sink.push(AgentEvent::RunCompleted { run_id, reason: "completed".into() });

    Ok((state, sink))
}

#[cfg(test)]
mod tests {
    use super::maybe_resume_from_interrupt;
    use agent_ports::{
        InterruptKind, InterruptSnapshot, ResumeCursor, StepSeq, ThreadId, ThreadState,
    };
    use serde_json::json;

    #[test]
    fn resume_clears_interrupt_and_clarification_when_user_input_present() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        st.pregel.interrupt = Some(InterruptSnapshot {
            kind: InterruptKind::Clarification { prompt: Some("why?".into()) },
            resume_cursor: ResumeCursor {
                node_id: crate::runtime_spec::DispatchPhaseNodes::CLARIFY.into(),
                superstep_seq: 1,
                step_seq: StepSeq::initial(),
            },
            payload: json!({}),
        });
        st.clarification_state.pending = true;
        st.clarification_state.prompt = Some("why?".into());

        maybe_resume_from_interrupt(&mut st, &[json!("answer")]);
        assert!(st.pregel.interrupt.is_none());
        assert!(st.pregel.last_resume_at.is_some());
        assert!(!st.clarification_state.pending);
        assert!(st.clarification_state.prompt.is_none());
    }

    #[test]
    fn resume_no_op_without_user_messages() {
        let tid = ThreadId::new_v4();
        let mut st = ThreadState::new(tid);
        st.pregel.interrupt = Some(InterruptSnapshot {
            kind: InterruptKind::Clarification { prompt: None },
            resume_cursor: ResumeCursor {
                node_id: crate::runtime_spec::DispatchPhaseNodes::CLARIFY.into(),
                superstep_seq: 0,
                step_seq: StepSeq::initial(),
            },
            payload: json!({}),
        });
        maybe_resume_from_interrupt(&mut st, &[]);
        assert!(st.pregel.interrupt.is_some());
    }
}
