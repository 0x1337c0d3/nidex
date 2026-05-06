use crate::auth::AuthMode;
use codex_protocol::openai_models::ModelUpgrade;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::default_input_modalities;
use once_cell::sync::Lazy;

pub const HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG: &str = "hide_gpt5_1_migration_prompt";
pub const HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG: &str =
    "hide_big-pickle_migration_prompt";

static PRESETS: Lazy<Vec<ModelPreset>> = Lazy::new(|| {
    vec![
        ModelPreset {
            id: "big-pickle".to_string(),
            model: "big-pickle".to_string(),
            display_name: "big-pickle".to_string(),
            description: "Latest frontier agentic coding model.".to_string(),
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
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
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
                migration_config_key: HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG.to_string(),
                model_link: None,
                upgrade_copy: None,
                migration_markdown: None,
            }),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
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
                migration_config_key: HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG.to_string(),
                model_link: None,
                upgrade_copy: None,
                migration_markdown: None,
            }),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
    ]
});

pub(super) fn builtin_model_presets(_auth_mode: Option<AuthMode>) -> Vec<ModelPreset> {
    PRESETS.iter().cloned().collect()
}

#[cfg(any(test, feature = "test-support"))]
pub fn all_model_presets() -> &'static Vec<ModelPreset> {
    &PRESETS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_default_model_is_configured() {
        let default_models = PRESETS.iter().filter(|preset| preset.is_default).count();
        assert!(default_models == 1);
    }
}
