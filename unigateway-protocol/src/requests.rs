use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use unigateway_core::{
    Message as CoreMessage, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest,
    ProxyResponsesRequest,
};

/// Translates an OpenAI-compatible JSON payload into a core `ProxyChatRequest`.
pub fn openai_payload_to_chat_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyChatRequest> {
    Ok(ProxyChatRequest {
        model: payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
        messages: chat_messages(payload)?,
        temperature: payload
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: payload
            .get("top_p")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        max_tokens: payload
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        stream: stream_flag(payload, false),
        metadata: HashMap::new(),
    })
}

/// Translates an OpenAI-compatible JSON payload into a core `ProxyResponsesRequest` (OpenAI Beta format).
pub fn openai_payload_to_responses_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyResponsesRequest> {
    let request = serde_json::from_value::<IncomingResponsesRequest>(payload.clone())
        .map_err(|error| anyhow!("failed to parse responses request: {error}"))?;

    Ok(ProxyResponsesRequest {
        model: request.model.unwrap_or_else(|| default_model.to_string()),
        input: request.input,
        instructions: request.instructions,
        temperature: request.temperature,
        top_p: request.top_p,
        max_output_tokens: request.max_output_tokens,
        stream: request.stream.unwrap_or(false),
        tools: request.tools,
        tool_choice: request.tool_choice,
        previous_response_id: request.previous_response_id,
        request_metadata: request.request_metadata,
        extra: filtered_response_extra(request.extra),
        metadata: HashMap::new(),
    })
}

/// Translates an Anthropic-compatible JSON payload into a core `ProxyChatRequest`.
pub fn anthropic_payload_to_chat_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyChatRequest> {
    let mut messages = Vec::new();
    if let Some(system) = payload.get("system").and_then(Value::as_str) {
        messages.push(CoreMessage {
            role: MessageRole::System,
            content: system.to_string(),
        });
    }
    messages.extend(chat_messages(payload)?);

    Ok(ProxyChatRequest {
        model: payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
        messages,
        temperature: payload
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: payload
            .get("top_p")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        max_tokens: payload
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        stream: stream_flag(payload, true),
        metadata: HashMap::new(),
    })
}

/// Translates an OpenAI-compatible JSON payload into a core `ProxyEmbeddingsRequest`.
pub fn openai_payload_to_embed_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyEmbeddingsRequest> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(default_model)
        .to_string();

    let input = match payload.get("input") {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect(),
        _ => return Err(anyhow!("input must be a string or array of strings")),
    };

    Ok(ProxyEmbeddingsRequest {
        model,
        input,
        encoding_format: payload
            .get("encoding_format")
            .and_then(Value::as_str)
            .map(String::from),
        metadata: HashMap::new(),
    })
}

fn stream_flag(payload: &Value, default: bool) -> bool {
    payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn chat_messages(payload: &Value) -> Result<Vec<CoreMessage>> {
    let Some(messages) = payload.get("messages").and_then(Value::as_array) else {
        return Err(anyhow!("messages must be an array"));
    };

    messages
        .iter()
        .map(|message| {
            let role = parse_role(
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user"),
            );
            let content = extract_text_content(
                message
                    .get("content")
                    .ok_or_else(|| anyhow!("message.content is required"))?,
            );
            Ok(CoreMessage { role, content })
        })
        .collect()
}

fn parse_role(role: &str) -> MessageRole {
    match role {
        "system" => MessageRole::System,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}

fn extract_text_content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }

    if let Some(blocks) = value.as_array() {
        let mut parts = Vec::new();
        for block in blocks {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
        return parts.join("\n");
    }

    String::new()
}

fn filtered_response_extra(extra: HashMap<String, Value>) -> HashMap<String, Value> {
    extra
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "target_vendor" | "target_provider" | "provider"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[derive(Debug, Deserialize)]
struct IncomingResponsesRequest {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    max_output_tokens: Option<u32>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    tools: Option<Value>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    previous_response_id: Option<String>,
    #[serde(default, rename = "metadata")]
    request_metadata: Option<Value>,
    #[serde(default, flatten)]
    extra: HashMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        anthropic_payload_to_chat_request, openai_payload_to_chat_request,
        openai_payload_to_embed_request, openai_payload_to_responses_request,
    };

    #[test]
    fn openai_requests_default_to_non_streaming() {
        let req = openai_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}]
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert!(!req.stream);
    }

    #[test]
    fn openai_requests_can_disable_streaming_explicitly() {
        let req = openai_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert!(!req.stream);
    }

    #[test]
    fn anthropic_requests_default_to_streaming() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}]
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert!(req.stream);
    }

    #[test]
    fn anthropic_requests_can_disable_streaming_explicitly() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}],
                "stream": false
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert!(!req.stream);
    }

    #[test]
    fn responses_extra_filter_strips_gateway_routing_hints_only() {
        let filtered = openai_payload_to_responses_request(
            &json!({
                "model": "gpt-4.1-mini",
                "input": "hello",
                "reasoning": {"effort": "high"},
                "target_provider": "deepseek",
                "provider": "moonshot"
            }),
            "gpt-4.1-mini",
        )
        .expect("request");

        assert!(filtered.extra.contains_key("reasoning"));
        assert!(!filtered.extra.contains_key("target_provider"));
        assert!(!filtered.extra.contains_key("provider"));
    }

    #[test]
    fn embeddings_conversion_preserves_encoding_format() {
        let converted = openai_payload_to_embed_request(
            &json!({
                "model": "text-embedding-3-small",
                "input": ["hello"],
                "encoding_format": "float"
            }),
            "text-embedding-3-small",
        )
        .expect("request");

        assert_eq!(converted.encoding_format.as_deref(), Some("float"));
    }
}
