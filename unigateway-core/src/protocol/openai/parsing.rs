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

/// Normalize upstream cache-hit token counts from heterogeneous OpenAI-compatible usage shapes.
///
/// Priority (first match wins; values are never summed):
/// 1. `usage.cache_hit_tokens`
/// 2. `usage.input_tokens_details.cached_tokens`
/// 3. `usage.prompt_tokens_details.cached_tokens`
/// 4. `usage.prompt_cache_hit_tokens`
/// 5. `usage.cached_tokens`
pub(super) fn parse_cache_hit_tokens(usage: &Value) -> Option<u64> {
    if let Some(value) = usage.get("cache_hit_tokens").and_then(Value::as_u64) {
        return Some(value);
    }
    if let Some(value) = usage
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
    {
        return Some(value);
    }
    if let Some(value) = usage
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
    {
        return Some(value);
    }
    if let Some(value) = usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64) {
        return Some(value);
    }
    usage.get("cached_tokens").and_then(Value::as_u64)
}

pub(super) fn parse_openai_usage(raw: &Value) -> Option<TokenUsage> {
    let usage = raw.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: None,
        cache_hit_tokens: parse_cache_hit_tokens(usage),
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

    let reasoning_tokens = usage
        .get("output_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .or_else(|| usage.get("reasoning_tokens"))
        .and_then(Value::as_u64);

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
        reasoning_tokens,
        cache_hit_tokens: parse_cache_hit_tokens(usage),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_cache_hit_tokens, parse_openai_usage, parse_responses_usage};

    #[test]
    fn parses_openai_chat_cached_tokens() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 100,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let usage = parse_openai_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(80));
    }

    #[test]
    fn parses_openai_responses_cached_tokens() {
        let raw = json!({
            "usage": {
                "input_tokens": 100,
                "input_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let usage = parse_responses_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(80));
    }

    #[test]
    fn parses_nested_responses_usage_cached_tokens() {
        let raw = json!({
            "response": {
                "usage": {
                    "input_tokens": 100,
                    "input_tokens_details": {
                        "cached_tokens": 80
                    }
                }
            }
        });

        let usage = parse_responses_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(80));
    }

    #[test]
    fn parses_deepseek_prompt_cache_hit_tokens() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 100,
                "prompt_cache_hit_tokens": 80,
                "prompt_cache_miss_tokens": 20
            }
        });

        let usage = parse_openai_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(80));
    }

    #[test]
    fn parses_qwen_top_level_cached_tokens() {
        let raw = json!({
            "usage": {
                "input_tokens": 100,
                "cached_tokens": 80
            }
        });

        let usage = parse_openai_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(80));
    }

    #[test]
    fn preserves_existing_cache_hit_tokens() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 10,
                "total_tokens": 30,
                "cache_hit_tokens": 12,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let usage = parse_openai_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, Some(12));
    }

    #[test]
    fn zero_cache_hit_tokens_is_some_zero() {
        let usage = json!({
            "prompt_tokens_details": {
                "cached_tokens": 0
            }
        });

        assert_eq!(parse_cache_hit_tokens(&usage), Some(0));
    }

    #[test]
    fn missing_cache_fields_is_none() {
        let raw = json!({
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 3,
                "total_tokens": 8
            }
        });

        let usage = parse_openai_usage(&raw).expect("usage parsed");
        assert_eq!(usage.cache_hit_tokens, None);
    }
}
