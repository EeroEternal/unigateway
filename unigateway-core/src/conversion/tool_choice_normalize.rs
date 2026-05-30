use serde_json::{Value, json};

use crate::capabilities::{ToolCallingCapabilities, ToolChoiceDowngradeTarget, ToolChoiceMode};
use crate::request::{
    TOOL_CHOICE_FORCE_OVERRIDE_KEY, TOOL_CHOICE_NORMALIZED_KEY, TOOL_CHOICE_ORIGINAL_KEY,
    TOOL_CHOICE_REACTIVE_RETRY_KEY, TOOL_CHOICE_REASON_KEY,
};

/// Result of applying endpoint tool-choice capabilities to an upstream payload field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolChoiceNormalization {
    pub original: Option<Value>,
    pub normalized: Option<Value>,
    pub reason: Option<String>,
}

/// Upstream wire protocol used when rendering a normalized `tool_choice` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamToolChoiceProtocol {
    OpenAi,
    Anthropic,
}

pub fn classify_openai_tool_choice(value: &Value) -> Option<ToolChoiceMode> {
    match value {
        Value::String(mode) => match mode.as_str() {
            "auto" => Some(ToolChoiceMode::Auto),
            "none" => Some(ToolChoiceMode::None),
            "required" => Some(ToolChoiceMode::Required),
            _ => None,
        },
        Value::Object(obj) => match obj.get("type").and_then(Value::as_str) {
            Some("function") => Some(ToolChoiceMode::NamedFunction),
            Some("auto") => Some(ToolChoiceMode::Auto),
            Some("none") => Some(ToolChoiceMode::None),
            Some("required") => Some(ToolChoiceMode::Required),
            _ => None,
        },
        _ => None,
    }
}

pub fn classify_anthropic_tool_choice(value: &Value) -> Option<ToolChoiceMode> {
    match value {
        Value::String(mode) => match mode.as_str() {
            "auto" => Some(ToolChoiceMode::Auto),
            "none" => Some(ToolChoiceMode::None),
            "required" => Some(ToolChoiceMode::Required),
            "any" => Some(ToolChoiceMode::Any),
            _ => None,
        },
        Value::Object(obj) => match obj.get("type").and_then(Value::as_str) {
            Some("auto") => Some(ToolChoiceMode::Auto),
            Some("none") => Some(ToolChoiceMode::None),
            Some("any") => Some(ToolChoiceMode::Any),
            Some("tool") => Some(ToolChoiceMode::NamedTool),
            Some("required") => Some(ToolChoiceMode::Required),
            _ => None,
        },
        _ => None,
    }
}

pub fn normalize_tool_choice(
    tool_choice: Option<Value>,
    tools: Option<&Value>,
    capabilities: &ToolCallingCapabilities,
    protocol: UpstreamToolChoiceProtocol,
) -> ToolChoiceNormalization {
    let Some(original) = tool_choice else {
        return ToolChoiceNormalization {
            original: None,
            normalized: None,
            reason: None,
        };
    };

    let classify = match protocol {
        UpstreamToolChoiceProtocol::OpenAi => classify_openai_tool_choice,
        UpstreamToolChoiceProtocol::Anthropic => classify_anthropic_tool_choice,
    };

    let Some(mode) = classify(&original) else {
        return ToolChoiceNormalization {
            original: Some(original.clone()),
            normalized: Some(original),
            reason: None,
        };
    };

    if capabilities.supported_modes.contains(&mode) {
        return ToolChoiceNormalization {
            original: Some(original.clone()),
            normalized: Some(original),
            reason: None,
        };
    }

    let Some(target) = capabilities.downgrade_policy.get(&mode).copied() else {
        return ToolChoiceNormalization {
            original: Some(original.clone()),
            normalized: Some(original),
            reason: Some(format!(
                "unsupported tool_choice mode {mode:?} with no downgrade policy"
            )),
        };
    };

    if capabilities.require_safe_downgrade && !is_safe_downgrade(&original, tools, mode) {
        return ToolChoiceNormalization {
            original: Some(original.clone()),
            normalized: Some(original),
            reason: Some("downgrade skipped: safe conditions not met".to_string()),
        };
    }

    let normalized = render_tool_choice_value(target, protocol, &original);
    ToolChoiceNormalization {
        original: Some(original),
        normalized: Some(normalized),
        reason: Some(format!("downgraded {mode:?} to {target:?}")),
    }
}

pub fn record_tool_choice_normalization(
    metadata: &mut std::collections::HashMap<String, String>,
    result: &ToolChoiceNormalization,
) {
    if result.original == result.normalized {
        return;
    }

    if let Some(original) = &result.original
        && let Ok(encoded) = serde_json::to_string(original)
    {
        metadata.insert(TOOL_CHOICE_ORIGINAL_KEY.to_string(), encoded);
    }
    if let Some(normalized) = &result.normalized
        && let Ok(encoded) = serde_json::to_string(normalized)
    {
        metadata.insert(TOOL_CHOICE_NORMALIZED_KEY.to_string(), encoded);
    }
    if let Some(reason) = &result.reason {
        metadata.insert(TOOL_CHOICE_REASON_KEY.to_string(), reason.clone());
    }
}

pub fn is_tool_choice_upstream_rejection(status: u16, body: Option<&str>) -> bool {
    if status != 400 {
        return false;
    }
    let Some(body) = body else {
        return false;
    };
    let lowered = body.to_ascii_lowercase();
    lowered.contains("tool_choice")
        || lowered.contains("tool choice")
        || lowered.contains("toolchoice")
        || lowered.contains("function_call")
        || lowered.contains("required is unsupported")
        || lowered.contains("unsupported tool")
}

pub fn reactive_tool_choice_fallback(
    current: Option<&Value>,
    protocol: UpstreamToolChoiceProtocol,
) -> Option<Value> {
    let current = current?;
    match protocol {
        UpstreamToolChoiceProtocol::OpenAi => match classify_openai_tool_choice(current)? {
            ToolChoiceMode::NamedFunction | ToolChoiceMode::Required => Some(json!("auto")),
            ToolChoiceMode::Auto => Some(json!("none")),
            _ => None,
        },
        UpstreamToolChoiceProtocol::Anthropic => match classify_anthropic_tool_choice(current)? {
            ToolChoiceMode::NamedTool => Some(json!({ "type": "any" })),
            ToolChoiceMode::Any | ToolChoiceMode::Required => Some(json!({ "type": "auto" })),
            ToolChoiceMode::Auto => Some(json!({ "type": "none" })),
            _ => None,
        },
    }
}

pub fn sent_tool_choice_from_metadata(
    metadata: &std::collections::HashMap<String, String>,
) -> Option<Value> {
    metadata
        .get(TOOL_CHOICE_NORMALIZED_KEY)
        .or_else(|| metadata.get(TOOL_CHOICE_ORIGINAL_KEY))
        .and_then(|encoded| serde_json::from_str(encoded).ok())
}

pub fn apply_reactive_tool_choice_override(
    metadata: &mut std::collections::HashMap<String, String>,
    next: &Value,
) {
    if let Ok(encoded) = serde_json::to_string(next) {
        metadata.insert(TOOL_CHOICE_FORCE_OVERRIDE_KEY.to_string(), encoded);
    }
    metadata.insert(
        TOOL_CHOICE_REACTIVE_RETRY_KEY.to_string(),
        "true".to_string(),
    );
    metadata.insert(
        TOOL_CHOICE_REASON_KEY.to_string(),
        "reactive tool_choice fallback retry".to_string(),
    );
}

pub fn resolve_upstream_tool_choice(
    endpoint: &mut std::collections::HashMap<String, String>,
    tool_choice: Option<Value>,
    tools: Option<&Value>,
    capabilities: &ToolCallingCapabilities,
    protocol: UpstreamToolChoiceProtocol,
) -> Option<Value> {
    if let Some(forced) = endpoint.get(TOOL_CHOICE_FORCE_OVERRIDE_KEY)
        && let Ok(value) = serde_json::from_str::<Value>(forced)
    {
        record_tool_choice_normalization(
            endpoint,
            &ToolChoiceNormalization {
                original: tool_choice.clone(),
                normalized: Some(value.clone()),
                reason: Some("forced tool_choice override".to_string()),
            },
        );
        return Some(value);
    }

    let normalization = normalize_tool_choice(tool_choice, tools, capabilities, protocol);
    record_tool_choice_normalization(endpoint, &normalization);
    normalization.normalized
}

fn is_safe_downgrade(original: &Value, tools: Option<&Value>, mode: ToolChoiceMode) -> bool {
    match mode {
        ToolChoiceMode::NamedFunction => {
            let Some(forced_name) = original.pointer("/function/name").and_then(Value::as_str)
            else {
                return false;
            };
            single_openai_tool_name(tools) == Some(forced_name)
        }
        ToolChoiceMode::NamedTool => {
            let Some(forced_name) = original.get("name").and_then(Value::as_str) else {
                return false;
            };
            single_anthropic_tool_name(tools) == Some(forced_name)
        }
        _ => true,
    }
}

fn single_openai_tool_name(tools: Option<&Value>) -> Option<&str> {
    let items = tools?.as_array()?;
    if items.len() != 1 {
        return None;
    }
    let tool = &items[0];
    if tool.get("type").and_then(Value::as_str) == Some("function") {
        return tool.pointer("/function/name").and_then(Value::as_str);
    }
    tool.get("name").and_then(Value::as_str)
}

fn single_anthropic_tool_name(tools: Option<&Value>) -> Option<&str> {
    let items = tools?.as_array()?;
    if items.len() != 1 {
        return None;
    }
    items[0].get("name").and_then(Value::as_str)
}

fn render_tool_choice_value(
    target: ToolChoiceDowngradeTarget,
    protocol: UpstreamToolChoiceProtocol,
    original: &Value,
) -> Value {
    match protocol {
        UpstreamToolChoiceProtocol::OpenAi => render_openai_tool_choice_value(target, original),
        UpstreamToolChoiceProtocol::Anthropic => {
            render_anthropic_tool_choice_value(target, original)
        }
    }
}

fn render_openai_tool_choice_value(target: ToolChoiceDowngradeTarget, _original: &Value) -> Value {
    match target {
        ToolChoiceDowngradeTarget::Auto => json!("auto"),
        ToolChoiceDowngradeTarget::None => json!("none"),
        ToolChoiceDowngradeTarget::Required => json!("required"),
        ToolChoiceDowngradeTarget::Any => json!("required"),
    }
}

fn render_anthropic_tool_choice_value(
    target: ToolChoiceDowngradeTarget,
    _original: &Value,
) -> Value {
    match target {
        ToolChoiceDowngradeTarget::Auto => json!({ "type": "auto" }),
        ToolChoiceDowngradeTarget::None => json!({ "type": "none" }),
        ToolChoiceDowngradeTarget::Required | ToolChoiceDowngradeTarget::Any => {
            json!({ "type": "any" })
        }
    }
}
