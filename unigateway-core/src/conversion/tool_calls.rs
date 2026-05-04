use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingOpenAiToolCall {
    pub id: String,
    pub name: String,
    pub raw_arguments: String,
    pub arguments: String,
    pub emitted_argument_len: usize,
    pub anthropic_index: Option<usize>,
    pub started: bool,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicToolUseStart {
    pub anthropic_index: usize,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicInputJsonDelta {
    pub anthropic_index: usize,
    pub partial_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiToolCallDeltaUpdate {
    pub start: Option<AnthropicToolUseStart>,
    pub delta: Option<AnthropicInputJsonDelta>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiToolCallStopUpdate {
    pub start: Option<AnthropicToolUseStart>,
    pub delta: Option<AnthropicInputJsonDelta>,
    pub stop_index: Option<usize>,
}

pub fn apply_openai_tool_call_delta_update(
    pending_tool_calls: &mut BTreeMap<usize, PendingOpenAiToolCall>,
    next_content_index: &mut usize,
    tool_index: usize,
    tool_call: &Value,
) -> OpenAiToolCallDeltaUpdate {
    pending_tool_calls.entry(tool_index).or_default();

    if pending_tool_calls
        .get(&tool_index)
        .and_then(|pending| pending.anthropic_index)
        .is_none()
    {
        let anthropic_index = *next_content_index;
        *next_content_index += 1;
        if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
            pending.anthropic_index = Some(anthropic_index);
        }
    }

    if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
            pending.id = id.to_string();
        }
        if let Some(name) = tool_call
            .get("function")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
        {
            pending.name = name.to_string();
        }
        if let Some(arguments) = tool_call
            .get("function")
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str)
        {
            pending.raw_arguments =
                merge_openai_tool_call_arguments(&pending.raw_arguments, arguments);
            pending.arguments =
                normalize_openai_tool_call_arguments(&pending.raw_arguments, &pending.arguments);
        }
    }

    let mut update = OpenAiToolCallDeltaUpdate::default();
    if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
        let anthropic_index = pending.anthropic_index.unwrap_or(tool_index);
        let can_start = !pending.started && !pending.id.is_empty() && !pending.name.is_empty();
        if can_start {
            pending.started = true;
            update.start = Some(AnthropicToolUseStart {
                anthropic_index,
                id: pending.id.clone(),
                name: pending.name.clone(),
            });
        }

        if pending.started && pending.emitted_argument_len < pending.arguments.len() {
            let fragment = pending.arguments[pending.emitted_argument_len..].to_string();
            pending.emitted_argument_len = pending.arguments.len();
            update.delta = Some(AnthropicInputJsonDelta {
                anthropic_index,
                partial_json: fragment,
            });
        }
    }

    update
}

pub fn flush_openai_tool_call_stop_update(
    pending_tool_calls: &mut BTreeMap<usize, PendingOpenAiToolCall>,
    tool_index: usize,
) -> OpenAiToolCallStopUpdate {
    let Some(pending) = pending_tool_calls.get_mut(&tool_index) else {
        return OpenAiToolCallStopUpdate::default();
    };

    if pending.stopped {
        return OpenAiToolCallStopUpdate::default();
    }

    let anthropic_index = pending.anthropic_index.unwrap_or(tool_index);

    if !pending.started {
        pending.started = true;

        if pending.id.is_empty() && pending.name.is_empty() && pending.arguments.is_empty() {
            return OpenAiToolCallStopUpdate::default();
        }

        let delta = if pending.emitted_argument_len < pending.arguments.len() {
            let partial_json = pending.arguments[pending.emitted_argument_len..].to_string();
            pending.emitted_argument_len = pending.arguments.len();
            Some(AnthropicInputJsonDelta {
                anthropic_index,
                partial_json,
            })
        } else {
            None
        };

        pending.stopped = true;
        return OpenAiToolCallStopUpdate {
            start: Some(AnthropicToolUseStart {
                anthropic_index,
                id: if pending.id.is_empty() {
                    "toolu_unknown".to_string()
                } else {
                    pending.id.clone()
                },
                name: if pending.name.is_empty() {
                    "tool".to_string()
                } else {
                    pending.name.clone()
                },
            }),
            delta,
            stop_index: Some(anthropic_index),
        };
    }

    pending.stopped = true;
    OpenAiToolCallStopUpdate {
        start: None,
        delta: None,
        stop_index: Some(anthropic_index),
    }
}

fn merge_openai_tool_call_arguments(existing: &str, incoming: &str) -> String {
    if incoming.is_empty() {
        return existing.to_string();
    }
    if existing.is_empty() {
        return incoming.to_string();
    }
    if incoming.starts_with(existing) {
        return incoming.to_string();
    }
    if existing.starts_with(incoming) || existing.ends_with(incoming) {
        return existing.to_string();
    }

    let max_overlap = existing.len().min(incoming.len());
    for overlap in (1..=max_overlap).rev() {
        if existing[existing.len() - overlap..] == incoming[..overlap] {
            return format!("{existing}{}", &incoming[overlap..]);
        }
    }

    format!("{existing}{incoming}")
}

fn normalize_openai_tool_call_arguments(raw_arguments: &str, previous_arguments: &str) -> String {
    if raw_arguments.is_empty() {
        return previous_arguments.to_string();
    }

    let mut normalized = raw_arguments.to_string();
    loop {
        let repaired = strip_empty_object_prefix(&normalized);
        if repaired != normalized {
            normalized = repaired;
            continue;
        }

        if normalized.starts_with('"') {
            match serde_json::from_str::<String>(&normalized) {
                Ok(decoded) => {
                    normalized = decoded;
                    continue;
                }
                Err(_) => return previous_arguments.to_string(),
            }
        }

        return normalized;
    }
}

fn strip_empty_object_prefix(value: &str) -> String {
    let mut stripped = value;
    while stripped.starts_with("{}") && stripped.len() > 2 {
        stripped = &stripped[2..];
    }
    stripped.to_string()
}
