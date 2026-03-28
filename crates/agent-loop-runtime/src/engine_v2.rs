//! Pregel-style superstep orchestration over the agent loop (V2 kernel).

use crate::agent_loop_types::{AgentLoopDeps, ToolLoopConfig};
use crate::budget::RunBudget;
use crate::dispatch::{execute_engine_command, route_llm_output, TurnDispatch};
use crate::error::{AgentLoopError, AgentLoopResult};
use crate::lead_kernel::apply_lead_kernel_turn;
use crate::loop_common::emit_stage;
use crate::loop_hardening::{apply_repeated_tool_loop_breaker, repair_missing_tool_results};
use crate::middleware::TurnContext;
use crate::pregel::bump_after_node;
use crate::run_config::AgentLoopRunConfig;
use crate::scheduler::{begin_outer_superstep, prepare_pull_task};
use crate::state_reducer::{append_user_messages, apply_run_config_bootstrap};
use agent_ports::{
    AgentEvent, EngineCommand, EventSink, LlmTurnContext, LoopStage, ThreadId, ThreadState,
};
use graph_runtime_core::GraphRuntime;
use serde_json::{json, Value};
use tracing::debug;

/// Run the main loop until finish or max turns (V2 Pregel-scheduled engine).
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

    append_user_messages(&mut state, &user_messages);
    apply_run_config_bootstrap(&mut state, run_cfg);
    state.migrate_to_latest_schema();

    let turn_ctx_base = TurnContext { thread_id, run_id, run_cfg: run_cfg.clone(), budget };

    let mut turns: u32 = 0;
    let mut last_tool_call_fingerprint: Option<u64> = None;
    loop {
        if turns >= budget.max_turns {
            return Err(AgentLoopError::MaxTurnsExceeded);
        }
        turns += 1;

        let step_seq = state.step_seq;

        begin_outer_superstep(&mut state);

        // --- Node: Lead ---
        prepare_pull_task(&mut state, "lead");
        if run_cfg.lead_spec.apply_lead_kernel {
            apply_lead_kernel_turn(deps.lead_kernel.as_ref(), &mut state, run_cfg).await?;
        }
        bump_after_node(&mut state, "lead");

        repair_missing_tool_results(&mut state);

        // --- Node: PreModel ---
        prepare_pull_task(&mut state, "premodel");
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PreModel, true);
        sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: agent_ports::StepKind::Llm });

        let mut injection = agent_ports::SkillInjection::default();
        if run_cfg.lead_spec.premodel_skills_memory {
            let skill_names = if run_cfg.skills_globally_enabled {
                run_cfg.enabled_skill_names.clone()
            } else {
                Vec::new()
            };
            injection = deps
                .skills
                .inject(&agent_ports::SkillContext { thread_id, enabled_skill_names: skill_names })
                .await?;
            if !injection.resolved_names.is_empty() {
                sink.push(AgentEvent::SkillInjected {
                    skill_names: injection.resolved_names.clone(),
                });
            }

            let mem_snippets = deps
                .memory
                .retrieve(&agent_ports::MemoryContext {
                    thread_id,
                    run_id,
                    query: "turn".into(),
                    state: state.clone(),
                })
                .await?;
            if !mem_snippets.is_empty() {
                state.memory_working_set.snippets = mem_snippets;
            }
        }
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PreModel, false);
        bump_after_node(&mut state, "premodel");

        let manifests = deps.tools.assemble(&tool_cfg.assembly);
        let assembled_tool_names: Vec<String> = manifests.iter().map(|m| m.name.clone()).collect();
        let mut messages_for_llm = build_llm_messages(&state, &injection.preamble, run_cfg);

        deps.middleware.before_model(&turn_ctx_base, &mut state, &mut messages_for_llm).await?;

        // --- Node: Model (execute) ---
        prepare_pull_task(&mut state, "model");
        emit_stage(&mut sink, run_id, step_seq, LoopStage::Model, true);
        let mut out = deps
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

        // --- Node: PostModel ---
        prepare_pull_task(&mut state, "postmodel");
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PostModel, true);
        deps.middleware.after_model(&turn_ctx_base, &mut state, &out).await?;
        emit_stage(&mut sink, run_id, step_seq, LoopStage::PostModel, false);
        bump_after_node(&mut state, "postmodel");

        apply_repeated_tool_loop_breaker(&mut out, &mut last_tool_call_fingerprint);

        sink.push(AgentEvent::LlmCompleted { run_id });
        sink.push(AgentEvent::StepFinished { run_id, step_seq, kind: agent_ports::StepKind::Llm });
        emit_stage(&mut sink, run_id, step_seq, LoopStage::Model, false);

        let cmd: EngineCommand = route_llm_output(&out);
        match execute_engine_command(
            graph,
            deps,
            &turn_ctx_base,
            thread_id,
            run_id,
            step_seq,
            &mut state,
            &mut sink,
            &manifests,
            cmd,
            &out,
            budget,
        )
        .await?
        {
            TurnDispatch::Stop => break,
            TurnDispatch::Again => continue,
        }
    }

    let finalize_seq = state.step_seq;
    emit_stage(&mut sink, run_id, finalize_seq, LoopStage::Finalize, true);
    let (_handle, state) = graph
        .complete_run(thread_id, run_id, state, "completed")
        .await
        .map_err(|e| AgentLoopError::Graph(e.to_string()))?;
    emit_stage(&mut sink, run_id, state.step_seq, LoopStage::Finalize, false);
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
