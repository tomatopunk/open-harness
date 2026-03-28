//! Lead / Subagent runtime spec：节点 id 与主超步 PULL 顺序契约（防分叉演化）。

use agent_loop_runtime::runtime_spec::{DispatchPhaseNodes, LeadRuntimeSpec, SubagentRuntimeSpec};

#[test]
fn lead_main_phase_pull_order_is_fixed_four_nodes() {
    let order = LeadRuntimeSpec::main_phase_pull_order();
    assert_eq!(
        order,
        &[
            LeadRuntimeSpec::NODE_LEAD,
            LeadRuntimeSpec::NODE_PREMODEL,
            LeadRuntimeSpec::NODE_MODEL,
            LeadRuntimeSpec::NODE_POSTMODEL,
        ]
    );
}

#[test]
fn dispatch_phase_nodes_are_distinct_from_lead_nodes() {
    assert_ne!(DispatchPhaseNodes::CLARIFY, LeadRuntimeSpec::NODE_LEAD);
    assert_ne!(DispatchPhaseNodes::TOOLS, SubagentRuntimeSpec::NODE_PREMODEL);
}
