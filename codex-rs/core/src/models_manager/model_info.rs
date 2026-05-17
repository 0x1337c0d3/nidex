use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::TruncationPolicyConfig;
use codex_protocol::openai_models::default_input_modalities;

use crate::config::Config;

pub const BASE_INSTRUCTIONS: &str = include_str!("../../prompt.md");
#[allow(dead_code)]
const BASE_INSTRUCTIONS_WITH_APPLY_PATCH: &str =
    include_str!("../../prompt_with_apply_patch_instructions.md");

const GEMINI4_INSTRUCTIONS: &str = include_str!("../../gemma-4_prompt.md");
const CODEX_INSTRUCTIONS: &str = include_str!("../../codex_prompt.md");

pub(crate) const CONTEXT_WINDOW_128K: i64 = 131_072;

macro_rules! model_info {
    (
        $slug:expr $(, $key:ident : $value:expr )* $(,)?
    ) => {{
        #[allow(unused_mut)]
        let mut model = ModelInfo {
            slug: $slug.to_string(),
            display_name: $slug.to_string(),
            description: None,
            // This is primarily used when remote metadata is available. When running
            // offline, core generally omits the effort field unless explicitly
            // configured by the user.
            default_reasoning_level: None,
            supported_reasoning_levels: supported_reasoning_level_low_medium_high(),
            shell_type: ConfigShellToolType::Default,
            visibility: ModelVisibility::None,
            supported_in_api: true,
            priority: 99,
            upgrade: None,
            base_instructions: BASE_INSTRUCTIONS.to_string(),
            model_messages: None,
            supports_reasoning_summaries: false,
            support_verbosity: false,
            default_verbosity: None,
            apply_patch_tool_type: None,
            truncation_policy: TruncationPolicyConfig::bytes(10_000),
            supports_parallel_tool_calls: false,
            context_window: Some(CONTEXT_WINDOW_128K),
            auto_compact_token_limit: None,
            effective_context_window_percent: 95,
            experimental_supported_tools: Vec::new(),
            input_modalities: default_input_modalities(),
            supported_message_roles: vec![
                "system".to_string(),
                "user".to_string(),
                "assistant".to_string(),
                "tool".to_string(),
            ],
            reasoning_field_name: None,
        };

        $(
            model.$key = $value;
        )*
        model
    }};
}

pub(crate) fn with_config_overrides(mut model: ModelInfo, config: &Config) -> ModelInfo {
    if let Some(supports_reasoning_summaries) = config.model_supports_reasoning_summaries {
        model.supports_reasoning_summaries = supports_reasoning_summaries;
    }
    if let Some(context_window) = config.model_context_window {
        model.context_window = Some(context_window);
    }
    if let Some(auto_compact_token_limit) = config.model_auto_compact_token_limit {
        model.auto_compact_token_limit = Some(auto_compact_token_limit);
    }
    if let Some(token_limit) = config.tool_output_token_limit {
        let limit = i64::try_from(token_limit).unwrap_or(i64::MAX);
        model.truncation_policy = TruncationPolicyConfig::tokens(limit);
    }

    if let Some(base_instructions) = &config.base_instructions {
        model.base_instructions = base_instructions.clone();
        model.model_messages = None;
    }

    model
}

// todo(aibrahim): remove most of the entries here when enabling models.json
pub(crate) fn find_model_info_for_slug(slug: &str) -> ModelInfo {
    let mut model = if slug.contains("gemma-4") {
        model_info!(
            slug,
            base_instructions: GEMINI4_INSTRUCTIONS.to_string(),
            shell_type: ConfigShellToolType::Default,
            supports_reasoning_summaries: true,
            support_verbosity: true,
            truncation_policy: TruncationPolicyConfig::bytes(10_000),
            context_window: Some(CONTEXT_WINDOW_128K),
        )
    } else {
        model_info!(
            slug,
            base_instructions: CODEX_INSTRUCTIONS.to_string(),
            shell_type: ConfigShellToolType::ShellCommand,
            supports_reasoning_summaries: true,
            support_verbosity: true,
            truncation_policy: TruncationPolicyConfig::bytes(10_000),
            context_window: Some(CONTEXT_WINDOW_128K),
        )
    };

    if slug == "test-big-pickle-codex" {
        model.experimental_supported_tools = vec![
            "test_sync_tool".to_string(),
            "read_file".to_string(),
            "grep_files".to_string(),
            "list_dir".to_string(),
        ];
    }

    model
}

fn supported_reasoning_level_low_medium_high() -> Vec<ReasoningEffortPreset> {
    vec![
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
    ]
}

#[allow(dead_code)]
fn supported_reasoning_level_low_medium_high_non_codex() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
        },
    ]
}

#[allow(dead_code)]
fn supported_reasoning_level_low_medium_high_xhigh() -> Vec<ReasoningEffortPreset> {
    vec![
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
    ]
}

#[allow(dead_code)]
fn supported_reasoning_level_low_medium_high_xhigh_non_codex() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::Medium,
            description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffort::XHigh,
            description: "Extra high reasoning for complex problems".to_string(),
        },
    ]
}
