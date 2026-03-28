//! ChildRun execution envelope (roadmap P5): budget + [`SubagentRuntimeSpec`] → [`agent_ports::SubagentExecuteParams`].

use crate::budget::RunBudget;
use crate::runtime_spec::SubagentRuntimeSpec;
use agent_ports::SubagentExecuteParams;

#[must_use]
pub(crate) fn subagent_params_for_child_run(
    budget: RunBudget,
    spec: &SubagentRuntimeSpec,
) -> SubagentExecuteParams {
    SubagentExecuteParams {
        max_concurrent: budget.max_concurrent_subagents.max(1),
        per_task_timeout: budget.per_subagent_task_timeout,
        inherit_premodel_skills_memory: spec.inherit_premodel_skills_memory,
    }
}
