use serde_json::Value;

use crate::error::GatewayError;
use crate::response::{ChatResponseFinal, TokenUsage};

pub fn parse_chat_response(
    body: &[u8],
) -> Result<(ChatResponseFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse anthropic chat response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty());

    let usage = parse_anthropic_usage(&raw);

    Ok((
        ChatResponseFinal {
            model: raw.get("model").and_then(Value::as_str).map(str::to_string),
            output_text,
            raw,
        },
        usage,
    ))
}

pub(super) fn parse_anthropic_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: match (
            usage.get("input_tokens").and_then(Value::as_u64),
            usage.get("output_tokens").and_then(Value::as_u64),
        ) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        },
    })
}
