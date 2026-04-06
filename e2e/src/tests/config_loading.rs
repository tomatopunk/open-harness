use std::fs;
use tempfile::tempdir;
use unified_config::loader::{load_unified_config, AppConfigRef, ModelConfigRef};

#[test]
fn test_config_loading_with_governance_files() {
    let temp_dir = tempdir().unwrap();
    let governance_root = temp_dir.path();

    fs::create_dir_all(governance_root).unwrap();

    fs::write(
        governance_root.join("models.yaml"),
        r#"
default_model: gpt-4
entries:
  - name: gpt-4
    provider: open_ai
"#,
    )
    .unwrap();

    fs::write(
        governance_root.join("policies.yaml"),
        r#"
policy_version: "1"
max_turns: 16
"#,
    )
    .unwrap();

    let app_config = AppConfigRef {
        models: vec![ModelConfigRef {
            name: "gpt-4".to_string(),
            display_name: "GPT-4".to_string(),
            use_provider: "open_ai".to_string(),
            model: "gpt-4".to_string(),
            api_key: None,
            max_tokens: None,
            temperature: None,
            base_url: None,
            use_responses_api: None,
            output_version: None,
        }],
        extensions_config_path: None,
    };

    let result = load_unified_config(&app_config, governance_root.to_str().unwrap());

    assert!(result.is_ok());

    let config = result.unwrap();
    assert_eq!(config.models.default_model, "gpt-4");
    assert_eq!(config.policies.max_turns, 16);
}

#[test]
fn test_config_loading_missing_governance_files_returns_defaults() {
    let temp_dir = tempdir().unwrap();
    let governance_root = temp_dir.path();

    let app_config = AppConfigRef {
        models: vec![ModelConfigRef {
            name: "gpt-4".to_string(),
            display_name: "GPT-4".to_string(),
            use_provider: "open_ai".to_string(),
            model: "gpt-4".to_string(),
            api_key: None,
            max_tokens: None,
            temperature: None,
            base_url: None,
            use_responses_api: None,
            output_version: None,
        }],
        extensions_config_path: None,
    };

    let result = load_unified_config(&app_config, governance_root.to_str().unwrap());

    assert!(result.is_ok());

    let config = result.unwrap();
    assert_eq!(config.policies.max_turns, 16);
    assert_eq!(config.subagents.max_concurrent, 4);
}

#[test]
fn test_config_validation_duplicate_models() {
    let temp_dir = tempdir().unwrap();
    let governance_root = temp_dir.path();

    let app_config = AppConfigRef {
        models: vec![
            ModelConfigRef {
                name: "gpt-4".to_string(),
                display_name: "GPT-4".to_string(),
                use_provider: "open_ai".to_string(),
                model: "gpt-4".to_string(),
                api_key: None,
                max_tokens: None,
                temperature: None,
                base_url: None,
                use_responses_api: None,
                output_version: None,
            },
            ModelConfigRef {
                name: "gpt-4".to_string(),
                display_name: "GPT-4 Duplicate".to_string(),
                use_provider: "open_ai".to_string(),
                model: "gpt-4".to_string(),
                api_key: None,
                max_tokens: None,
                temperature: None,
                base_url: None,
                use_responses_api: None,
                output_version: None,
            },
        ],
        extensions_config_path: None,
    };

    let result = load_unified_config(&app_config, governance_root.to_str().unwrap());

    assert!(result.is_err());
}
