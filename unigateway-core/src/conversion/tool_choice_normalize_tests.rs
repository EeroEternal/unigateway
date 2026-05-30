use serde_json::json;

use crate::capabilities::{ToolCallingCapabilities, ToolChoiceDowngradeTarget, ToolChoiceMode};
use crate::conversion::{
    UpstreamToolChoiceProtocol, is_tool_choice_upstream_rejection, normalize_tool_choice,
    reactive_tool_choice_fallback, record_tool_choice_normalization,
};
use crate::request::{
    TOOL_CHOICE_NORMALIZED_KEY, TOOL_CHOICE_ORIGINAL_KEY, TOOL_CHOICE_REASON_KEY,
};

#[test]
fn normalize_openai_named_function_downgrades_to_auto_by_default() {
    let tools = json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {}}
        }
    }]);
    let tool_choice = json!({
        "type": "function",
        "function": {"name": "get_weather"}
    });

    let result = normalize_tool_choice(
        Some(tool_choice.clone()),
        Some(&tools),
        &ToolCallingCapabilities::openai_compatible_default(),
        UpstreamToolChoiceProtocol::OpenAi,
    );

    assert_eq!(result.normalized, Some(json!("auto")));
    assert_eq!(result.original, Some(tool_choice));
    assert!(result.reason.is_some());
}

#[test]
fn normalize_openai_required_downgrades_to_auto_by_default() {
    let tools = json!([{
        "type": "function",
        "function": {"name": "get_weather"}
    }]);

    let result = normalize_tool_choice(
        Some(json!("required")),
        Some(&tools),
        &ToolCallingCapabilities::openai_compatible_default(),
        UpstreamToolChoiceProtocol::OpenAi,
    );

    assert_eq!(result.normalized, Some(json!("auto")));
}

#[test]
fn normalize_memtensor_style_named_function_downgrades_to_required() {
    let tools = json!([{
        "type": "function",
        "function": {"name": "get_weather"}
    }]);
    let tool_choice = json!({
        "type": "function",
        "function": {"name": "get_weather"}
    });

    let result = normalize_tool_choice(
        Some(tool_choice),
        Some(&tools),
        &ToolCallingCapabilities::memtensor_style(),
        UpstreamToolChoiceProtocol::OpenAi,
    );

    assert_eq!(result.normalized, Some(json!("required")));
}

#[test]
fn normalize_skips_named_function_downgrade_when_tool_name_mismatch() {
    let tools = json!([{
        "type": "function",
        "function": {"name": "other_tool"}
    }]);
    let tool_choice = json!({
        "type": "function",
        "function": {"name": "get_weather"}
    });

    let result = normalize_tool_choice(
        Some(tool_choice.clone()),
        Some(&tools),
        &ToolCallingCapabilities::openai_compatible_default(),
        UpstreamToolChoiceProtocol::OpenAi,
    );

    assert_eq!(result.normalized, Some(tool_choice));
    assert_eq!(
        result.reason.as_deref(),
        Some("downgrade skipped: safe conditions not met")
    );
}

#[test]
fn normalize_anthropic_named_tool_downgrades_to_any_when_configured() {
    let tools = json!([{
        "name": "get_weather",
        "input_schema": {"type": "object", "properties": {}}
    }]);
    let tool_choice = json!({"type": "tool", "name": "get_weather"});
    let capabilities = ToolCallingCapabilities {
        supported_modes: vec![
            ToolChoiceMode::Auto,
            ToolChoiceMode::None,
            ToolChoiceMode::Any,
        ],
        downgrade_policy: std::collections::HashMap::from([(
            ToolChoiceMode::NamedTool,
            ToolChoiceDowngradeTarget::Any,
        )]),
        streaming_tool_calls: true,
        require_safe_downgrade: true,
    };

    let result = normalize_tool_choice(
        Some(tool_choice),
        Some(&tools),
        &capabilities,
        UpstreamToolChoiceProtocol::Anthropic,
    );

    assert_eq!(result.normalized, Some(json!({"type": "any"})));
}

#[test]
fn reactive_tool_choice_fallback_steps_down_openai_chain() {
    assert_eq!(
        reactive_tool_choice_fallback(
            Some(&json!({"type": "function", "function": {"name": "get_weather"}})),
            UpstreamToolChoiceProtocol::OpenAi,
        ),
        Some(json!("auto"))
    );
    assert_eq!(
        reactive_tool_choice_fallback(Some(&json!("auto")), UpstreamToolChoiceProtocol::OpenAi),
        Some(json!("none"))
    );
}

#[test]
fn is_tool_choice_upstream_rejection_matches_common_provider_errors() {
    assert!(is_tool_choice_upstream_rejection(
        400,
        Some("tool_choice required is unsupported")
    ));
    assert!(!is_tool_choice_upstream_rejection(
        400,
        Some("invalid model name")
    ));
}

#[test]
fn record_tool_choice_normalization_writes_metadata_keys() {
    let mut metadata = std::collections::HashMap::new();
    let result = normalize_tool_choice(
        Some(json!({
            "type": "function",
            "function": {"name": "get_weather"}
        })),
        Some(&json!([{
            "type": "function",
            "function": {"name": "get_weather"}
        }])),
        &ToolCallingCapabilities::openai_compatible_default(),
        UpstreamToolChoiceProtocol::OpenAi,
    );

    record_tool_choice_normalization(&mut metadata, &result);

    assert!(metadata.contains_key(TOOL_CHOICE_ORIGINAL_KEY));
    assert!(metadata.contains_key(TOOL_CHOICE_NORMALIZED_KEY));
    assert!(metadata.contains_key(TOOL_CHOICE_REASON_KEY));
}
