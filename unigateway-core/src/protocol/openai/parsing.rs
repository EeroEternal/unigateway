use serde_json::Value;

use crate::error::GatewayError;
use crate::response::{ChatResponseFinal, EmbeddingsResponse, ResponsesFinal, TokenUsage};

pub fn parse_chat_response(
    body: &[u8],
) -> Result<(ChatResponseFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai chat response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(super::super::output_text_from_openai_message);

    let usage = parse_openai_usage(&raw);

    Ok((
        ChatResponseFinal {
            model: raw.get("model").and_then(Value::as_str).map(str::to_string),
            output_text,
            raw,
        },
        usage,
    ))
}

pub fn parse_responses_response(
    body: &[u8],
) -> Result<(ResponsesFinal, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai responses response: {error}"),
        endpoint_id: None,
    })?;

    let output_text = raw
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_responses_output_text(&raw));

    let usage = parse_responses_usage(&raw);

    Ok((ResponsesFinal { output_text, raw }, usage))
}

pub fn parse_embeddings_response(
    body: &[u8],
) -> Result<(EmbeddingsResponse, Option<TokenUsage>), GatewayError> {
    let raw: Value = serde_json::from_slice(body).map_err(|error| GatewayError::Transport {
        message: format!("failed to parse openai embeddings response: {error}"),
        endpoint_id: None,
    })?;
    let usage = parse_openai_usage(&raw);
    Ok((EmbeddingsResponse { raw }, usage))
}

pub(super) fn parse_openai_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
}

fn extract_responses_output_text(raw: &Value) -> Option<String> {
    raw.get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            let texts = items
                .iter()
                .flat_map(|item| {
                    item.get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(|content| content.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();

            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        })
}

pub(super) fn parse_responses_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw
        .get("response")
        .and_then(|response| response.get("usage"))
        .or_else(|| raw.get("usage"))?;

    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
    })
}
