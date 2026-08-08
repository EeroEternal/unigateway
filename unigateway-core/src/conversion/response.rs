use serde_json::{Value, json};

use crate::error::GatewayError;
use crate::request::ContentBlock;
use crate::response::TokenUsage;

use super::blocks::{anthropic_blocks, anthropic_content_to_blocks};

/// Maps an Anthropic `stop_reason` to an OpenAI `finish_reason`.
pub fn map_anthropic_stop_reason_to_finish_reason(stop_reason: &str) -> &'static str {
    match stop_reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        _ => "stop",
    }
}

/// Converts Anthropic message `content` into an OpenAI assistant `message` object.
pub fn anthropic_content_to_openai_assistant_message(
    content: &Value,
) -> Result<Value, GatewayError> {
    let blocks = anthropic_content_to_blocks(content)?;
    let mut text_parts = Vec::new();
    let mut thinking_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => text_parts.push(text),
            ContentBlock::Thinking { thinking, .. } => thinking_parts.push(thinking),
            ContentBlock::ToolUse { .. } => {
                if let Some(tool_call) = block.to_openai_tool_call()? {
                    tool_calls.push(tool_call);
                }
            }
            ContentBlock::Image { .. } | ContentBlock::ToolResult { .. } => {}
        }
    }

    let mut message =
        serde_json::Map::from_iter([("role".to_string(), Value::String("assistant".to_string()))]);

    let content_text = text_parts.join("\n");
    if !content_text.is_empty() {
        message.insert("content".to_string(), Value::String(content_text));
    } else if tool_calls.is_empty() {
        message.insert("content".to_string(), Value::Null);
    }

    if !thinking_parts.is_empty() {
        let thinking = thinking_parts.join("\n");
        message.insert(
            "reasoning_content".to_string(),
            Value::String(thinking.clone()),
        );
        message.insert("thinking".to_string(), Value::String(thinking));
    }

    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    Ok(Value::Object(message))
}

/// Builds an OpenAI `chat.completion` body from a raw Anthropic message payload.
pub fn anthropic_message_to_openai_chat_completion(
    raw: &Value,
    request_id: &str,
    model: Option<&str>,
    report_usage: Option<&TokenUsage>,
) -> Result<Value, GatewayError> {
    let content = raw.get("content").cloned().unwrap_or(Value::Null);
    let message = anthropic_content_to_openai_assistant_message(&content)?;

    let openai_id = raw
        .get("id")
        .and_then(Value::as_str)
        .map(|id| {
            if id.starts_with("msg_") {
                id.strip_prefix("msg_").unwrap_or(id).to_string()
            } else {
                id.to_string()
            }
        })
        .unwrap_or_else(|| request_id.to_string());

    let finish_reason = raw
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_anthropic_stop_reason_to_finish_reason)
        .or_else(|| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .filter(|calls| !calls.is_empty())
                .map(|_| "tool_calls")
        })
        .unwrap_or("stop");

    let usage = raw
        .get("usage")
        .map(anthropic_usage_value_to_openai_usage)
        .or_else(|| report_usage.map(token_usage_to_openai_usage));

    Ok(json!({
        "id": openai_id,
        "object": "chat.completion",
        "model": model
            .map(str::to_string)
            .or_else(|| raw.get("model").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default(),
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason,
        }],
        "usage": usage,
    }))
}

/// Returns true when `raw` looks like an Anthropic assistant message payload.
pub fn is_anthropic_message_raw(raw: &Value) -> bool {
    if raw.get("choices").is_some() {
        return false;
    }
    if raw.get("type").and_then(Value::as_str) == Some("message") {
        return raw
            .get("content")
            .is_some_and(has_anthropic_convertible_content);
    }
    raw.get("content")
        .is_some_and(has_anthropic_convertible_content)
}

fn has_anthropic_convertible_content(content: &Value) -> bool {
    anthropic_blocks(content.clone()).iter().any(|block| {
        matches!(
            block.get("type").and_then(Value::as_str),
            Some("text" | "thinking" | "tool_use" | "tool_result" | "image")
        )
    })
}

pub fn anthropic_usage_value_to_openai_usage(usage: &Value) -> Value {
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut object = serde_json::Map::from_iter([
        ("prompt_tokens".to_string(), json!(input_tokens)),
        ("completion_tokens".to_string(), json!(output_tokens)),
        (
            "total_tokens".to_string(),
            json!(input_tokens + output_tokens),
        ),
    ]);

    for (anthropic_key, openai_key) in [
        ("cache_creation_input_tokens", "cache_creation_input_tokens"),
        ("cache_read_input_tokens", "cache_read_input_tokens"),
    ] {
        if let Some(value) = usage.get(anthropic_key) {
            object.insert(openai_key.to_string(), value.clone());
        }
    }

    Value::Object(object)
}

fn token_usage_to_openai_usage(usage: &TokenUsage) -> Value {
    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let mut object = serde_json::Map::from_iter([
        ("prompt_tokens".to_string(), json!(input)),
        ("completion_tokens".to_string(), json!(output)),
        (
            "total_tokens".to_string(),
            json!(usage.total_tokens.unwrap_or(input + output)),
        ),
    ]);
    if let Some(cache_hit) = usage.cache_hit_tokens {
        object.insert("cache_hit_tokens".to_string(), json!(cache_hit));
    }
    Value::Object(object)
}
