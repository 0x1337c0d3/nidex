use anyhow::Result;
use codex_core::CodexAuth;
use codex_core::ThreadManager;
use codex_core::built_in_model_providers;
use codex_core::models_manager::manager::RefreshStrategy;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelUpgrade;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::default_input_modalities;
use core_test_support::load_default_config_for_test;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_returns_api_key_models() -> Result<()> {
    let codex_home = tempdir()?;
    let config = load_default_config_for_test(&codex_home).await;
    let manager = ThreadManager::with_models_provider(
        CodexAuth::from_api_key("sk-test"),
        built_in_model_providers()["openai"].clone(),
    );
    let models = manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;

    let expected_models = expected_models_for_api_key();
    assert_eq!(expected_models, models);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_returns_chatgpt_models() -> Result<()> {
    let codex_home = tempdir()?;
    let config = load_default_config_for_test(&codex_home).await;
    let manager = ThreadManager::with_models_provider(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
        built_in_model_providers()["openai"].clone(),
    );
    let models = manager
        .list_models(&config, RefreshStrategy::OnlineIfUncached)
        .await;

    let expected_models = expected_models_for_chatgpt();
    assert_eq!(expected_models, models);

    Ok(())
}

fn expected_models_for_api_key() -> Vec<ModelPreset> {
    vec![
        builtin_big_pickle(),
        builtin_big_pickle_codex(),
        builtin_big_pickle_codex_mini(),
    ]
}

fn expected_models_for_chatgpt() -> Vec<ModelPreset> {
    expected_models_for_api_key()
}

fn builtin_big_pickle() -> ModelPreset {
    ModelPreset {
        id: "big-pickle".to_string(),
        model: "big-pickle".to_string(),
        display_name: "big-pickle".to_string(),
        description: "big pickle".to_string(),
        default_reasoning_effort: ReasoningEffort::Medium,
        supported_reasoning_efforts: vec![
            ReasoningEffortPreset {
                effort: ReasoningEffort::Low,
                description: "Fast responses with lighter reasoning".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description: "Dynamically adjusts reasoning based on the task".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::High,
                description: "Maximizes reasoning depth for complex or ambiguous problems"
                    .to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::XHigh,
                description: "Extra high reasoning depth for complex problems".to_string(),
            },
        ],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        input_modalities: default_input_modalities(),
    }
}

fn builtin_big_pickle_codex() -> ModelPreset {
    ModelPreset {
        id: "big-pickle-codex".to_string(),
        model: "big-pickle-codex".to_string(),
        display_name: "big-pickle-codex".to_string(),
        description: "Advanced coding model with extended context.".to_string(),
        default_reasoning_effort: ReasoningEffort::Medium,
        supported_reasoning_efforts: vec![
            ReasoningEffortPreset {
                effort: ReasoningEffort::Low,
                description: "Fast responses with lighter reasoning".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description: "Balances speed and reasoning depth for everyday tasks".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::High,
                description: "Greater reasoning depth for complex problems".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::XHigh,
                description: "Extra high reasoning depth for complex problems".to_string(),
            },
        ],
        is_default: false,
        upgrade: Some(ModelUpgrade {
            id: "big-pickle".to_string(),
            reasoning_effort_mapping: None,
            migration_config_key: "hide_gpt5_1_migration_prompt".to_string(),
            model_link: None,
            upgrade_copy: None,
            migration_markdown: None,
        }),
        show_in_picker: false,
        supported_in_api: true,
        input_modalities: default_input_modalities(),
    }
}

fn builtin_big_pickle_codex_mini() -> ModelPreset {
    ModelPreset {
        id: "big-pickle-codex-mini".to_string(),
        model: "big-pickle-codex-mini".to_string(),
        display_name: "big-pickle-codex-mini".to_string(),
        description: "Compact coding model for everyday tasks.".to_string(),
        default_reasoning_effort: ReasoningEffort::Medium,
        supported_reasoning_efforts: vec![
            ReasoningEffortPreset {
                effort: ReasoningEffort::Low,
                description: "Fast responses with lighter reasoning".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description: "Balances speed and reasoning depth for everyday tasks".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffort::High,
                description: "Greater reasoning depth for complex problems".to_string(),
            },
        ],
        is_default: false,
        upgrade: Some(ModelUpgrade {
            id: "big-pickle".to_string(),
            reasoning_effort_mapping: None,
            migration_config_key: "hide_gpt5_1_migration_prompt".to_string(),
            model_link: None,
            upgrade_copy: None,
            migration_markdown: None,
        }),
        show_in_picker: false,
        supported_in_api: true,
        input_modalities: default_input_modalities(),
    }
}
