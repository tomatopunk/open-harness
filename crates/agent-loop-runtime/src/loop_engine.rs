//! Multi-turn model-tool-state loop.

use std::sync::Arc;

use crate::budget::RunBudget;
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::run_config::AgentLoopRunConfig;
use agent_ports::{
    tool_allowed, AgentEvent, EventSink, LLMPort, LlmTurnContext, MemoryPort, SkillPort, StepKind,
    SubagentPort, ThreadId, ThreadState, ToolAssemblyPolicy, ToolPort,
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
    run_cfg: &AgentLoopRunConfig,
) -> AgentLoopResult<(ThreadState, EventSink)> {
    let mut sink = EventSink::default();
    let run = graph
        .start_run(thread_id, state.clone())
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    let run_id = run.run_id;
    sink.push(AgentEvent::RunStarted { thread_id, run_id });

    for msg in &user_messages {
        state.messages.push(agent_ports::ChatMessage { role: "user".into(), content: msg.clone() });
    }

    if !run_cfg.policy_version.is_empty() {
        state.governance_marks.policy_version = Some(run_cfg.policy_version.clone());
    }
    if run_cfg.loop_detected {
        state.governance_marks.tags.push("loop_detected".into());
    }
    for t in &run_cfg.seed_todos {
        state.todos.push(t.clone());
    }
    if run_cfg.is_plan_mode {
        state.plan_state.active = true;
    }

    let mut turns: u32 = 0;
    loop {
        if turns >= budget.max_turns {
            return Err(AgentLoopError::MaxTurnsExceeded);
        }
        turns += 1;

        let step_seq = state.step_seq;
        sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: StepKind::Llm });

        let skill_names = if run_cfg.skills_globally_enabled {
            run_cfg.enabled_skill_names.clone()
        } else {
            Vec::new()
        };
        let injection = deps
            .skills
            .inject(&agent_ports::SkillContext { thread_id, enabled_skill_names: skill_names })
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
        let assembled_tool_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
        let messages_for_llm = build_llm_messages(&state, &injection.preamble, run_cfg);

        let out = deps
            .llm
            .infer_turn(LlmTurnContext {
                run_id,
                thread_id,
                messages: messages_for_llm,
                system_prompt: Some(injection.preamble),
                model_name: run_cfg.model_name.clone(),
                policy_version: Some(run_cfg.policy_version.clone()).filter(|s| !s.is_empty()),
                is_plan_mode: run_cfg.is_plan_mode,
                assembled_tool_names,
                loop_detected: run_cfg.loop_detected,
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
            let (committed_seq, committed) = graph
                .commit_step(thread_id, run_id, state.clone(), json!({ "reason": "clarification" }))
                .await
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            state = committed;
            sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });
            sink.push(AgentEvent::RunCompleted { run_id, reason: "clarification".into() });
            return Ok((state, sink));
        }

        if let Some(plan) = &out.subtask_plan {
            let max_t = budget.max_subagent_tasks as usize;
            let truncated: agent_ports::SubtaskPlan = if plan.tasks.len() > max_t {
                warn!("subtask plan truncated by budget");
                agent_ports::SubtaskPlan { tasks: plan.tasks.iter().take(max_t).cloned().collect() }
            } else {
                plan.clone()
            };
            sink.push(AgentEvent::SubagentPlanned { run_id, task_count: truncated.tasks.len() });
            let results =
                deps.subagents.execute_plan(run_id, thread_id, &truncated, &state).await?;
            for (i, r) in results.iter().enumerate() {
                sink.push(AgentEvent::SubagentCompleted { run_id, task_id: r.task_id });
                let goal = truncated.tasks.get(i).map(|t| t.goal.clone()).unwrap_or_default();
                state.subagent_tasks.push(agent_ports::SubagentTaskRecord {
                    task_id: r.task_id,
                    goal,
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
            let (committed_seq, committed) = graph
                .commit_step(thread_id, run_id, state, serde_json::json!({}))
                .await
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            state = committed;
            sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });
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
                    payload: payload.clone(),
                });
                state.messages.push(agent_ports::ChatMessage {
                    role: "tool".into(),
                    content: json!({
                        "tool_call_id": call.call_id,
                        "name": call.name,
                        "content": payload
                    }),
                });
            }
            let (committed_seq, committed) = graph
                .commit_step(thread_id, run_id, state, serde_json::json!({}))
                .await
                .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
            state = committed;
            sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });
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

        let (committed_seq, committed) = graph
            .commit_step(thread_id, run_id, state, serde_json::json!({}))
            .await
            .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
        state = committed;
        sink.push(AgentEvent::StateCommitted { run_id, step_seq: committed_seq });

        if out.finish_turn {
            break;
        }
    }

    let (_handle, state) = graph
        .complete_run(thread_id, run_id, state, "completed")
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    sink.push(AgentEvent::RunCompleted { run_id, reason: "completed".into() });

    Ok((state, sink))
}

fn build_llm_messages(
    state: &ThreadState,
    preamble: &str,
    run_cfg: &AgentLoopRunConfig,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut system = String::new();
    if !preamble.is_empty() {
        system.push_str(preamble);
    }
    if run_cfg.is_plan_mode && !system.is_empty() {
        system.push_str("\nPlan mode: maintain an explicit todo list and complete steps in order.");
    } else if run_cfg.is_plan_mode {
        system.push_str("Plan mode: maintain an explicit todo list and complete steps in order.");
    }
    if !system.is_empty() {
        out.push(json!({
            "role": "system",
            "content": system
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
