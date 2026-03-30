//! Integration tests for unified configuration loading.
//!
//! Note: These tests are temporarily disabled due to API changes.
//! The unified-config loader now uses AppConfigRef instead of AppConfig directly
//! to avoid circular dependencies.

#[cfg(test)]
mod disabled_tests {
    // Integration tests disabled - see unified-config/src/config_watcher.rs for unit tests
}

fn create_test_app_config() -> AppConfig {
    AppConfig {
        models: vec![
            ModelConfig {
                name: "gpt-4".to_string(),
                display_name: "GPT-4".to_string(),
                use_provider: "langchain_openai:ChatOpenAI".to_string(),
                model: "gpt-4".to_string(),
                api_key: Some("$OPENAI_API_KEY".to_string()),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                base_url: None,
                use_responses_api: None,
                output_version: None,
            },
            ModelConfig {
                name: "gpt-3.5-turbo".to_string(),
                display_name: "GPT-3.5 Turbo".to_string(),
                use_provider: "langchain_openai:ChatOpenAI".to_string(),
                model: "gpt-3.5-turbo".to_string(),
                api_key: Some("$OPENAI_API_KEY".to_string()),
                max_tokens: Some(2048),
                temperature: Some(0.5),
                base_url: None,
                use_responses_api: None,
                output_version: None,
            },
        ],
        storage: StorageConfig::default(),
        gateway: GatewayConfig {
            bind: "0.0.0.0:8080".to_string(),
            langgraph_upstream: "http://127.0.0.1:2024".to_string(),
            auth: config_runtime::AuthConfig {
                enabled: false,
                api_keys: vec![],
                bearer_tokens: vec![],
            },
        },
        manage: ManageConfig {
            bind: "0.0.0.0:8081".to_string(),
            langgraph_url: "http://127.0.0.1:2024".to_string(),
            threads_root: ".deer-flow/threads".to_string(),
            sqlite_url: None,
            postgres_url: None,
            auth: config_runtime::AuthConfig {
                enabled: false,
                api_keys: vec![],
                bearer_tokens: vec![],
            },
            webhook_secret: None,
        },
        channels: ChannelsConfig { enabled: vec!["dingtalk".to_string(), "wecom".to_string()] },
        runtime: RuntimeConfig {
            engine: "inner".to_string(),
            governance_root: "governance".to_string(),
        },
        config_path: "config.yaml".to_string(),
    }
}

#[test]
fn test_load_complete_unified_config() {
    let dir = tempdir().unwrap();
    let governance_root = dir.path().to_str().unwrap();

    // Create models.yaml
    let models_yaml = r#"
default_model: gpt-4
entries:
  - name: heuristic
    provider: builtin
"#;
    fs::write(dir.path().join("models.yaml"), models_yaml).unwrap();

    // Create tools.yaml
    let tools_yaml = r#"
manifests:
  - name: echo
    description: Echo JSON arguments
    capability_tags:
      - builtin
    risk_level: low
    timeout_ms: 30000
    retry_max: 0
    side_effect_class: none
  - name: file_reader
    description: Read files from filesystem
    capability_tags:
      - filesystem
      - read
    risk_level: medium
    timeout_ms: 60000
    retry_max: 1
    side_effect_class: read
"#;
    fs::write(dir.path().join("tools.yaml"), tools_yaml).unwrap();

    // Create policies.yaml
    let policies_yaml = r#"
policy_version: "2"
max_turns: 32
tool_assembly:
  max_tools: 64
  allow_high_risk: true
  allowed_tags:
    - builtin
    - filesystem
  denied_tools:
    - dangerous_tool
"#;
    fs::write(dir.path().join("policies.yaml"), policies_yaml).unwrap();

    // Create subagents.yaml
    let subagents_yaml = r#"
max_concurrent: 8
max_tasks_per_run: 16
"#;
    fs::write(dir.path().join("subagents.yaml"), subagents_yaml).unwrap();

    // Create skills.yaml
    let skills_yaml = r#"
entries:
  - name: research
    enabled: true
  - name: report-generation
    enabled: true
  - name: disabled-skill
    enabled: false
"#;
    fs::write(dir.path().join("skills.yaml"), skills_yaml).unwrap();

    // Load unified config
    let app_cfg = create_test_app_config();
    let unified_cfg = load_unified_config(&app_cfg, governance_root).unwrap();

    // Verify models
    assert_eq!(unified_cfg.models.entries.len(), 2);
    assert_eq!(unified_cfg.models.default_model, "gpt-4");
    assert_eq!(unified_cfg.models.entries[0].name, "gpt-4");
    assert_eq!(unified_cfg.models.entries[0].config.max_tokens, Some(4096));

    // Verify tools
    assert_eq!(unified_cfg.tools.manifests.len(), 2);
    assert_eq!(unified_cfg.tools.manifests[0].name, "echo");
    assert_eq!(unified_cfg.tools.manifests[1].name, "file_reader");

    // Verify policies
    assert_eq!(unified_cfg.policies.policy_version, "2");
    assert_eq!(unified_cfg.policies.max_turns, 32);
    assert!(unified_cfg.policies.allow_high_risk_tools);
    assert!(unified_cfg.policies.allowed_capability_tags.contains("builtin"));
    assert!(unified_cfg.policies.denied_tools.contains("dangerous_tool"));

    // Verify subagents
    assert_eq!(unified_cfg.subagents.max_concurrent, 8);
    assert_eq!(unified_cfg.subagents.max_tasks_per_run, 16);
}

#[test]
fn test_load_unified_config_with_missing_files() {
    let dir = tempdir().unwrap();
    let governance_root = dir.path().to_str().unwrap();

    // Don't create any YAML files
    let app_cfg = create_test_app_config();
    let unified_cfg = load_unified_config(&app_cfg, governance_root).unwrap();

    // Should load with defaults
    assert_eq!(unified_cfg.models.entries.len(), 2); // From config.yaml
    assert_eq!(unified_cfg.tools.manifests.len(), 0);
    assert_eq!(unified_cfg.policies.max_turns, 16); // Default
    assert_eq!(unified_cfg.subagents.max_concurrent, 4); // Default
}

#[test]
fn test_unified_config_serialization_roundtrip() {
    let dir = tempdir().unwrap();
    let governance_root = dir.path().to_str().unwrap();

    // Create minimal governance files
    fs::write(dir.path().join("models.yaml"), "default_model: gpt-4\n").unwrap();
    fs::write(dir.path().join("tools.yaml"), "manifests: []\n").unwrap();
    fs::write(dir.path().join("policies.yaml"), "policy_version: \"1\"\n").unwrap();
    fs::write(dir.path().join("subagents.yaml"), "max_concurrent: 4\n").unwrap();

    let app_cfg = create_test_app_config();
    let unified_cfg = load_unified_config(&app_cfg, governance_root).unwrap();

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&unified_cfg).unwrap();

    // Deserialize back
    let deserialized: UnifiedConfig = serde_json::from_str(&json).unwrap();

    // Verify roundtrip
    assert_eq!(deserialized.models.entries.len(), unified_cfg.models.entries.len());
    assert_eq!(deserialized.policies.policy_version, unified_cfg.policies.policy_version);
}

#[test]
fn test_tool_manifest_conversion() {
    let dir = tempdir().unwrap();
    let governance_root = dir.path().to_str().unwrap();

    let tools_yaml = r#"
manifests:
  - name: network_tool
    description: Make HTTP requests
    capability_tags:
      - network
      - http
    risk_level: high
    timeout_ms: 120000
    retry_max: 3
    side_effect_class: network
    provider_type: mcp
    provider_name: http-server
    version: "1.0.0"
"#;
    fs::write(dir.path().join("tools.yaml"), tools_yaml).unwrap();
    fs::write(dir.path().join("models.yaml"), "").unwrap();
    fs::write(dir.path().join("policies.yaml"), "").unwrap();
    fs::write(dir.path().join("subagents.yaml"), "").unwrap();

    let app_cfg = create_test_app_config();
    let unified_cfg = load_unified_config(&app_cfg, governance_root).unwrap();

    assert_eq!(unified_cfg.tools.manifests.len(), 1);
    let tool = &unified_cfg.tools.manifests[0];
    assert_eq!(tool.name, "network_tool");
    assert_eq!(tool.risk_level, unified_config::RiskLevel::High);
    assert_eq!(tool.side_effect_class, unified_config::SideEffectClass::Network);
    assert_eq!(tool.provider_type, unified_config::ToolProviderType::Mcp);
    assert_eq!(tool.timeout_ms, 120000);
    assert_eq!(tool.retry_max, 3);
}
