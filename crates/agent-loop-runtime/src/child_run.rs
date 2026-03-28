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

#[cfg(test)]
mod tests {
    use super::subagent_params_for_child_run;
    use crate::budget::RunBudget;
    use crate::runtime_spec::SubagentRuntimeSpec;
    use std::time::Duration;

    #[test]
    fn params_reflect_budget_and_spec() {
        let budget = RunBudget {
            max_concurrent_subagents: 0,
            per_subagent_task_timeout: Some(Duration::from_secs(30)),
            ..RunBudget::default()
        };
        let spec = SubagentRuntimeSpec { inherit_premodel_skills_memory: true };
        let p = subagent_params_for_child_run(budget, &spec);
        assert_eq!(p.max_concurrent, 1);
        assert_eq!(p.per_task_timeout, Some(Duration::from_secs(30)));
        assert!(p.inherit_premodel_skills_memory);
    }

    /// R3：高并发下参数推导仍确定、无 silent 放宽并发下限。
    #[test]
    fn params_deterministic_under_repeated_calls() {
        let budget = RunBudget { max_concurrent_subagents: 16, ..RunBudget::default() };
        let spec = SubagentRuntimeSpec::default();
        for _ in 0..500 {
            let p = subagent_params_for_child_run(budget, &spec);
            assert_eq!(p.max_concurrent, 16);
        }
    }
}
