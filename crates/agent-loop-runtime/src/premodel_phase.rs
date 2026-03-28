//! Shared PreModel skill + memory injection (Lead and Subagent runtime spec).

use crate::error::AgentLoopResult;
use crate::loop_common::emit_stage;
use crate::middleware::TurnContext;
use crate::run_config::AgentLoopRunConfig;
use agent_ports::{
    AgentEvent, EventSink, LoopStage, MemoryContext, SkillContext, SkillInjection, ThreadId,
    ThreadState,
};

/// Run skill injection + memory retrieve when `enabled` (Lead `premodel_skills_memory` or Subagent inherit).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_premodel_skills_memory(
    deps: &crate::agent_loop_types::AgentLoopDeps,
    _turn_ctx: &TurnContext,
    thread_id: ThreadId,
    run_id: agent_ports::RunId,
    step_seq: agent_ports::StepSeq,
    state: &mut ThreadState,
    sink: &mut EventSink,
    run_cfg: &AgentLoopRunConfig,
    enabled: bool,
) -> AgentLoopResult<SkillInjection> {
    let mut injection = SkillInjection::default();
    if !enabled {
        return Ok(injection);
    }

    emit_stage(sink, run_id, step_seq, LoopStage::PreModel, true);
    sink.push(AgentEvent::StepStarted { run_id, step_seq, kind: agent_ports::StepKind::Llm });

    let skill_names = if run_cfg.skills_globally_enabled {
        run_cfg.enabled_skill_names.clone()
    } else {
        Vec::new()
    };
    injection =
        deps.skills.inject(&SkillContext { thread_id, enabled_skill_names: skill_names }).await?;
    if !injection.resolved_names.is_empty() {
        sink.push(AgentEvent::SkillInjected { skill_names: injection.resolved_names.clone() });
    }

    let mem_snippets = deps
        .memory
        .retrieve(&MemoryContext { thread_id, run_id, query: "turn".into(), state: state.clone() })
        .await?;
    if !mem_snippets.is_empty() {
        state.memory_working_set.snippets = mem_snippets;
    }

    emit_stage(sink, run_id, step_seq, LoopStage::PreModel, false);
    Ok(injection)
}
