//! Multi-turn model-tool-state loop.

use std::sync::Arc;

use crate::budget::RunBudget;
use crate::error::{AgentLoopError, AgentLoopResult};
use agent_ports::{
    tool_allowed, AgentEvent, CheckpointPort, EventSink, LLMPort, LlmTurnContext, MemoryPort,
    SkillPort, StepKind, SubagentPort, ThreadId, ThreadState, ToolAssemblyPolicy, ToolPort,
};
use graph_runtime_core::GraphRuntime;
use serde_json::{json, Value};
use tracing::{debug, warn};

/// Bundles ports for one agent loop execution.
pub struct AgentLoopDeps {
    pub llm: Arc<dyn LLMPort>,
    pub tools: Arc<dyn ToolPort>,
    pub memory: Arc<dyn MemoryPort>,
    pub skills: Arc<dyn SkillPort>,
    pub subagents: Arc<dyn SubagentPort>,
    pub checkpoints: Arc<dyn CheckpointPort>,
}

/// Configuration for dynamic tool assembly (from governance).
#[derive(Debug, Clone, Default)]
pub struct ToolLoopConfig {
    pub assembly: ToolAssemblyPolicy,
}

/// Run the main loop until finish or max turns.
pub async fn run_agent_loop(
    graph: &GraphRuntime,
    deps: &AgentLoopDeps,
    thread_id: ThreadId,
    mut state: ThreadState,
    user_messages: Vec<Value>,
    budget: RunBudget,
    tool_cfg: &ToolLoopConfig,
) -> AgentLoopResult<(ThreadState, EventSink)> {
    let mut sink = EventSink::default();
    let run = graph
        .start_run(thread_id, state.clone())
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    let run_id = run.run_id;
    sink.push(AgentEvent::RunStarted { thread_id, run_id });

    for msg in &user_messages {
        state.messages.push(agent_ports::ChatMessage { role: "user".into(), content: msg.clone() });
    }

    let mut turns: u32 = 0;
    loop {
        if turns >= budget.max_turns {
            return Err(AgentLoopError::MaxTurnsExceeded);
        }
        turns += 1;

        let step_seq = state.step_seq;
        sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: StepKind::Llm });

        let injection = deps
            .skills
            .inject(&agent_ports::SkillContext { thread_id, enabled_skill_names: Vec::new() })
            .await?;
        if !injection.resolved_names.is_empty() {
            sink.push(AgentEvent::SkillInjected { skill_names: injection.resolved_names.clone() });
        }

        let mem_snippets = deps
            .memory
            .retrieve(&agent_ports::MemoryContext {
                thread_id,
                run_id,
                query: "turn".into(),
                state: state.clone(),
            })
            .await
            .unwrap_or_default();
        if !mem_snippets.is_empty() {
            state.memory_working_set.snippets = mem_snippets;
        }

        let manifests = deps.tools.assemble(&tool_cfg.assembly);
        let messages_for_llm = build_llm_messages(&state, &injection.preamble);

        let out = deps
            .llm
            .infer_turn(LlmTurnContext {
                run_id,
                thread_id,
                messages: messages_for_llm,
                system_prompt: Some(injection.preamble),
                model_name: None,
            })
            .await?;

        sink.push(AgentEvent::LlmCompleted { run_id });
        sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: StepKind::Llm });

        if out.needs_clarification {
            state.clarification_state.pending = true;
            state.clarification_state.prompt = out.clarification_prompt.clone();
            sink.push(AgentEvent::ClarificationRequested {
                run_id,
                prompt: out.clarification_prompt.clone(),
            });
            graph
                .commit_step(thread_id, run_id, state.clone())
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            let cp = agent_ports::CheckpointRecord {
                id: agent_ports::CheckpointId::new_v4(),
                thread_id,
                run_id,
                step_seq: state.step_seq,
                state: state.clone(),
                metadata: json!({"reason": "clarification"}),
            };
            deps.checkpoints.save(cp).await?;
            sink.push(AgentEvent::RunCompleted { run_id, reason: "clarification".into() });
            return Ok((state, sink));
        }

        if let Some(plan) = &out.subtask_plan {
            if plan.tasks.len() > budget.max_subagent_tasks as usize {
                warn!("subtask plan truncated by budget");
            }
            sink.push(AgentEvent::SubagentPlanned {
                run_id,
                task_count: plan.tasks.len().min(budget.max_subagent_tasks as usize),
            });
            let results = deps.subagents.execute_plan(run_id, thread_id, plan, &state).await?;
            for r in &results {
                sink.push(AgentEvent::SubagentCompleted { run_id, task_id: r.task_id });
                state.subagent_tasks.push(agent_ports::SubagentTaskRecord {
                    task_id: r.task_id,
                    goal: plan.tasks.first().map(|t| t.goal.clone()).unwrap_or_default(),
                    status: if r.ok { "ok".into() } else { "failed".into() },
                    output: Some(r.output.clone()),
                });
            }
            let merged = deps
                .subagents
                .merge(
                    &agent_ports::SubagentMergeContext { thread_id, run_id, state: state.clone() },
                    &results,
                )
                .await?;
            state = merged;
            graph
                .commit_step(thread_id, run_id, state.clone())
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            sink.push(AgentEvent::StateCommitted { run_id, step_seq: state.step_seq });
            if out.finish_turn {
                break;
            }
            continue;
        }

        if !out.tool_calls.is_empty() {
            for call in &out.tool_calls {
                if !tool_allowed(&call.name, &manifests) {
                    warn!("tool not in assembled manifest: {}", call.name);
                    continue;
                }
                sink.push(AgentEvent::ToolSelected { run_id, tool_name: call.name.clone() });
                let res = deps.tools.invoke(run_id, thread_id, call).await;
                let ok = res.is_ok();
                let payload = res.unwrap_or_else(|e| json!({ "error": e.to_string() }));
                sink.push(AgentEvent::ToolExecuted { run_id, tool_name: call.name.clone(), ok });
                state.tool_invocations.push(agent_ports::ToolInvocationRecord {
                    tool_name: call.name.clone(),
                    args: call.args.clone(),
                    invocation_id: call.call_id.clone(),
                });
                state.tool_results.push(agent_ports::ToolResultRecord {
                    invocation_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    ok,
                    payload,
                });
            }
            graph
                .commit_step(thread_id, run_id, state.clone())
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            sink.push(AgentEvent::StateCommitted { run_id, step_seq: state.step_seq });
            if out.finish_turn {
                break;
            }
            continue;
        }

        if let Some(text) = out.assistant_text {
            state.messages.push(agent_ports::ChatMessage {
                role: "assistant".into(),
                content: Value::String(text),
            });
        }

        let _mem = deps.memory.extract_and_commit(&mut state).await?;
        sink.push(AgentEvent::MemoryUpdated);

        graph
            .commit_step(thread_id, run_id, state.clone())
            .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
        sink.push(AgentEvent::StateCommitted { run_id, step_seq: state.step_seq });

        if out.finish_turn {
            break;
        }
    }

    let cp = agent_ports::CheckpointRecord {
        id: agent_ports::CheckpointId::new_v4(),
        thread_id,
        run_id,
        step_seq: state.step_seq,
        state: state.clone(),
        metadata: json!({}),
    };
    deps.checkpoints.save(cp).await?;

    let _ = graph
        .complete_run(thread_id, run_id, state.clone(), "completed")
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    sink.push(AgentEvent::RunCompleted { run_id, reason: "completed".into() });

    Ok((state, sink))
}

fn build_llm_messages(state: &ThreadState, preamble: &str) -> Vec<Value> {
    let mut out = Vec::new();
    if !preamble.is_empty() {
        out.push(json!({
            "role": "system",
            "content": preamble
        }));
    }
    for m in &state.messages {
        out.push(json!({
            "role": m.role,
            "content": m.content
        }));
    }
    debug!(count = out.len(), "llm messages built");
    out
}
