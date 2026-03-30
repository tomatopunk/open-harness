//! Integration tests for result merger strategies.

use agent_ports::{
    ConcatenateMerger, ConsensusConfig, ConsensusMerger, MergeContext, ResultMerger,
    SubagentResult, VotingMethod,
};
use agent_ports::{RunId, ThreadId};
use serde_json::json;
use uuid::Uuid;

fn create_test_result(ok: bool, output: serde_json::Value) -> SubagentResult {
    SubagentResult { task_id: Uuid::new_v4(), ok, output }
}

fn create_merge_context() -> MergeContext {
    MergeContext::new(
        ThreadId::new_v4(),
        RunId::new_v4(),
        agent_ports::ThreadState::default(),
        agent_ports::SubtaskPlan::default(),
        None,
    )
}

#[tokio::test]
async fn test_concatenate_merger() {
    let merger = ConcatenateMerger::with_default_config();
    let results = vec![
        create_test_result(true, json!({"data": "result1"})),
        create_test_result(true, json!({"data": "result2"})),
        create_test_result(false, json!({"error": "failed"})),
    ];

    let ctx = create_merge_context();
    let merged = merger.merge(&ctx, &results).await.unwrap();

    assert!(merged.summary.is_some());
    assert!(merged.content.is_string());
    let content = merged.content.as_str().unwrap();
    assert!(content.contains("result1"));
    assert!(content.contains("result2"));
}

#[tokio::test]
async fn test_consensus_majority_vote() {
    let config =
        ConsensusConfig { voting_method: VotingMethod::Majority, threshold: 0.5, vote_field: None };
    let merger = ConsensusMerger::new(config);

    // Create results where "option_a" should win by majority
    let results = vec![
        create_test_result(true, json!("option_a")),
        create_test_result(true, json!("option_a")),
        create_test_result(true, json!("option_b")),
    ];

    let ctx = create_merge_context();
    let merged = merger.merge(&ctx, &results).await.unwrap();

    assert!(merged.summary.is_some());
    let metadata = &merged.metadata;
    assert!(metadata.get("consensus_reached").unwrap().as_bool().unwrap());
    assert_eq!(metadata.get("winner").unwrap().as_str().unwrap(), "option_a");
}

#[tokio::test]
async fn test_consensus_weighted_vote() {
    let config =
        ConsensusConfig { voting_method: VotingMethod::Weighted, threshold: 0.5, vote_field: None };
    let merger = ConsensusMerger::new(config);

    // Create results with confidence scores
    let results = vec![
        create_test_result(true, json!({"choice": "option_a", "confidence": 0.9})),
        create_test_result(true, json!({"choice": "option_a", "confidence": 0.8})),
        create_test_result(true, json!({"choice": "option_b", "confidence": 0.5})),
    ];

    let ctx = create_merge_context();
    let merged = merger.merge(&ctx, &results).await.unwrap();

    assert!(merged.summary.is_some());
    let metadata = &merged.metadata;
    assert!(metadata.get("voting_method").unwrap().as_str().unwrap() == "weighted");
}

#[tokio::test]
async fn test_consensus_approval_vote() {
    let config =
        ConsensusConfig { voting_method: VotingMethod::Approval, threshold: 0.5, vote_field: None };
    let merger = ConsensusMerger::new(config);

    // Create results with approval status
    let results = vec![
        create_test_result(true, json!({"approve": true, "option": "A"})),
        create_test_result(true, json!({"approve": true, "option": "A"})),
        create_test_result(true, json!({"approve": false, "option": "A"})),
    ];

    let ctx = create_merge_context();
    let merged = merger.merge(&ctx, &results).await.unwrap();

    assert!(merged.summary.is_some());
    let metadata = &merged.metadata;
    assert_eq!(metadata.get("voting_method").unwrap().as_str().unwrap(), "approval");
}

#[tokio::test]
async fn test_consensus_threshold_not_met() {
    let config = ConsensusConfig {
        voting_method: VotingMethod::Majority,
        threshold: 0.8, // High threshold
        vote_field: None,
    };
    let merger = ConsensusMerger::new(config);

    // Create results with no clear majority
    let results = vec![
        create_test_result(true, json!("option_a")),
        create_test_result(true, json!("option_b")),
        create_test_result(true, json!("option_c")),
    ];

    let ctx = create_merge_context();
    let merged = merger.merge(&ctx, &results).await.unwrap();

    let metadata = &merged.metadata;
    assert!(!metadata.get("consensus_reached").unwrap().as_bool().unwrap());
}

#[test]
fn test_voting_method_serialization() {
    // Test that voting methods can be serialized/deserialized
    let methods = vec![
        VotingMethod::Majority,
        VotingMethod::Weighted,
        VotingMethod::Borda,
        VotingMethod::Approval,
    ];

    for method in methods {
        let config = ConsensusConfig { voting_method: method, threshold: 0.5, vote_field: None };
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: ConsensusConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            format!("{:?}", config.voting_method),
            format!("{:?}", deserialized.voting_method)
        );
    }
}
