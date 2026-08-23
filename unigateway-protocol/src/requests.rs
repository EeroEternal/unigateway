use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::Value;
use unigateway_core::{
    ClientProtocol, ContentBlock, Message as CoreMessage, MessageRole, ProxyChatRequest,
    ProxyEmbeddingsRequest, ProxyResponsesRequest, ThinkingSignatureStatus,
    anthropic_content_to_blocks, is_placeholder_thinking_signature,
    normalize_proxy_responses_request, openai_message_to_content_blocks,
};

pub const ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY: &str = "unigateway.requested_model_alias";

/// Translates an OpenAI-compatible JSON payload into a core `ProxyChatRequest`.
pub fn openai_payload_to_chat_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyChatRequest> {
    let raw_messages = payload.get("messages").cloned();
    let mut request = ProxyChatRequest {
        model: payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
        messages: openai_chat_messages(payload)?,
        temperature: payload
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: payload
            .get("top_p")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_k: payload
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        max_tokens: payload
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        stop_sequences: payload.get("stop").cloned(),
        stream: stream_flag(payload, false),
        system: None,
        tools: payload.get("tools").cloned(),
        tool_choice: payload.get("tool_choice").cloned(),
        raw_messages,
        gateway_fields: openai_chat_gateway_fields(payload),
        extra: openai_chat_extra(payload),
        metadata: HashMap::new(),
    };

    if request.raw_messages.is_some() {
        request.mark_openai_raw_messages();
    }
    request.set_client_protocol(ClientProtocol::OpenAiChat);
    request.set_thinking_signature_status(openai_thinking_signature_status(
        request.raw_messages.as_ref(),
    ));

    Ok(request)
}

/// Translates an OpenAI-compatible JSON payload into a core `ProxyResponsesRequest` (OpenAI Beta format).
pub fn openai_payload_to_responses_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyResponsesRequest> {
    let request = serde_json::from_value::<IncomingResponsesRequest>(payload.clone())
        .map_err(|error| anyhow!("failed to parse responses request: {error}"))?;

    let mut proxy = ProxyResponsesRequest {
        model: request.model.unwrap_or_else(|| default_model.to_string()),
        input: request.input,
        instructions: request.instructions,
        temperature: request.temperature,
        top_p: request.top_p,
        max_output_tokens: request.max_output_tokens,
        stream: request.stream.unwrap_or(false),
        tools: request.tools,
        tool_choice: request.tool_choice,
        reasoning: request.reasoning,
        previous_response_id: request.previous_response_id,
        request_metadata: request.request_metadata,
        extra: filtered_response_extra(request.extra),
        metadata: HashMap::new(),
    };
    normalize_proxy_responses_request(&mut proxy);
    Ok(proxy)
}

/// Translates an Anthropic-compatible JSON payload into a core `ProxyChatRequest`.
pub fn anthropic_payload_to_chat_request(
    payload: &Value,
    default_model: &str,
) -> Result<ProxyChatRequest> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(default_model)
        .to_string();
    let mut messages = Vec::new();
    if let Some(system) = payload.get("system").and_then(Value::as_str) {
        messages.push(CoreMessage::text(MessageRole::System, system));
    }
    messages.extend(anthropic_chat_messages(payload)?);

    let mut request = ProxyChatRequest {
        model: model.clone(),
        messages,
        temperature: payload
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_p: payload
            .get("top_p")
            .and_then(Value::as_f64)
            .map(|v| v as f32),
        top_k: payload
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        max_tokens: payload
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32),
        stop_sequences: payload.get("stop_sequences").cloned(),
        stream: stream_flag(payload, true),
        system: payload.get("system").cloned(),
        tools: payload.get("tools").cloned(),
        tool_choice: payload.get("tool_choice").cloned(),
        raw_messages: payload.get("messages").cloned(),
        gateway_fields: anthropic_chat_gateway_fields(payload),
        extra: anthropic_chat_extra(payload),
        metadata: anthropic_requested_model_alias(model),
    };

    request.set_client_protocol(ClientProtocol::AnthropicMessages);
    request.set_thinking_signature_status(anthropic_thinking_signature_status(
        request.raw_messages.as_ref(),
    ));

    Ok(request)
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

pub fn anthropic_requested_model_alias(model: String) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    set_anthropic_requested_model_alias(&mut metadata, model);
    metadata.insert(
        unigateway_core::CLIENT_PROTOCOL_KEY.to_string(),
        ClientProtocol::AnthropicMessages
            .as_metadata_value()
            .to_string(),
    );
    metadata
}

pub fn set_anthropic_requested_model_alias(
    metadata: &mut HashMap<String, String>,
    model: impl Into<String>,
) {
    metadata.insert(
        ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY.to_string(),
        model.into(),
    );
}

pub fn anthropic_requested_model_alias_from_metadata(
    metadata: &HashMap<String, String>,
) -> Option<&str> {
    metadata
        .get(ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY)
        .map(String::as_str)
}

pub fn anthropic_requested_model_alias_or(
    metadata: &HashMap<String, String>,
    fallback: &str,
) -> String {
    anthropic_requested_model_alias_from_metadata(metadata)
        .unwrap_or(fallback)
        .to_string()
}

fn openai_chat_messages(payload: &Value) -> Result<Vec<CoreMessage>> {
    chat_messages(payload, |message| {
        openai_message_to_content_blocks(message).map_err(|error| anyhow!(error.to_string()))
    })
}

fn anthropic_chat_messages(payload: &Value) -> Result<Vec<CoreMessage>> {
    chat_messages(payload, |message| {
        let content = message
            .get("content")
            .ok_or_else(|| anyhow!("message.content is required"))?;
        anthropic_content_to_blocks(content).map_err(|error| anyhow!(error.to_string()))
    })
}

fn chat_messages(
    payload: &Value,
    content_blocks: impl Fn(&Value) -> Result<Vec<ContentBlock>>,
) -> Result<Vec<CoreMessage>> {
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
            Ok(CoreMessage::from_blocks(role, content_blocks(message)?))
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

fn openai_chat_extra(payload: &Value) -> HashMap<String, Value> {
    split_chat_payload_extensions(payload, is_openai_chat_known_field).extra
}

fn openai_chat_gateway_fields(payload: &Value) -> HashMap<String, Value> {
    split_chat_payload_extensions(payload, is_openai_chat_known_field).gateway_fields
}

fn anthropic_chat_extra(payload: &Value) -> HashMap<String, Value> {
    split_chat_payload_extensions(payload, is_anthropic_chat_known_field).extra
}

fn anthropic_chat_gateway_fields(payload: &Value) -> HashMap<String, Value> {
    split_chat_payload_extensions(payload, is_anthropic_chat_known_field).gateway_fields
}

struct ChatPayloadExtensions {
    extra: HashMap<String, Value>,
    gateway_fields: HashMap<String, Value>,
}

fn split_chat_payload_extensions(
    payload: &Value,
    is_known_field: fn(&str) -> bool,
) -> ChatPayloadExtensions {
    let Some(object) = payload.as_object() else {
        return ChatPayloadExtensions {
            extra: HashMap::new(),
            gateway_fields: HashMap::new(),
        };
    };

    let mut extra = HashMap::new();
    let mut gateway_fields = HashMap::new();
    for (key, value) in object {
        if is_known_field(key) {
            continue;
        }
        if unigateway_core::is_gateway_only_field_key(key) {
            gateway_fields.insert(key.clone(), value.clone());
        } else {
            extra.insert(key.clone(), value.clone());
        }
    }

    ChatPayloadExtensions {
        extra,
        gateway_fields,
    }
}

fn is_openai_chat_known_field(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "messages"
            | "temperature"
            | "top_p"
            | "top_k"
            | "max_tokens"
            | "stop"
            | "stream"
            | "tools"
            | "tool_choice"
            | "target_vendor"
            | "target_provider"
            | "provider"
    )
}

fn is_anthropic_chat_known_field(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "messages"
            | "temperature"
            | "top_p"
            | "top_k"
            | "max_tokens"
            | "stop_sequences"
            | "stream"
            | "system"
            | "tools"
            | "tool_choice"
    )
}

fn openai_thinking_signature_status(raw_messages: Option<&Value>) -> ThinkingSignatureStatus {
    let Some(messages) = raw_messages.and_then(Value::as_array) else {
        return ThinkingSignatureStatus::Absent;
    };

    if messages.iter().any(|message| {
        message
            .get("reasoning_content")
            .or_else(|| message.get("thinking"))
            .and_then(Value::as_str)
            .is_some_and(|thinking| !thinking.is_empty())
    }) {
        ThinkingSignatureStatus::Placeholder
    } else {
        ThinkingSignatureStatus::Absent
    }
}

fn anthropic_thinking_signature_status(raw_messages: Option<&Value>) -> ThinkingSignatureStatus {
    let Some(messages) = raw_messages.and_then(Value::as_array) else {
        return ThinkingSignatureStatus::Absent;
    };

    let mut has_verbatim = false;
    for message in messages {
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            continue;
        };

        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("thinking") {
                continue;
            }

            let Some(signature) = block
                .get("signature")
                .and_then(Value::as_str)
                .filter(|signature| !signature.is_empty())
            else {
                continue;
            };

            if is_placeholder_thinking_signature(signature) {
                return ThinkingSignatureStatus::Placeholder;
            }
            has_verbatim = true;
        }
    }

    if has_verbatim {
        ThinkingSignatureStatus::Verbatim
    } else {
        ThinkingSignatureStatus::Absent
    }
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
    reasoning: Option<Value>,
    #[serde(default)]
    previous_response_id: Option<String>,
    #[serde(default, rename = "metadata")]
    request_metadata: Option<Value>,
    #[serde(default, flatten)]
    extra: HashMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;
    use unigateway_core::{
        ClientProtocol, THINKING_SIGNATURE_PLACEHOLDER_VALUE, ThinkingSignatureStatus,
    };

    use super::{
        ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY, anthropic_payload_to_chat_request,
        openai_payload_to_chat_request, openai_payload_to_embed_request,
        openai_payload_to_responses_request,
    };

    #[test]
    fn openai_requests_default_to_non_streaming() {
        let req = openai_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}],
                "stop": ["DONE"]
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert!(!req.stream);
        assert_eq!(req.stop_sequences, Some(json!(["DONE"])));
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
    fn openai_chat_extra_preserves_unknown_fields_only() {
        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-5.5",
                "messages": [{"role": "user", "content": "hello"}],
                "reasoning_effort": "high",
                "max_completion_tokens": 1024,
                "target_provider": "internal",
                "provider": "internal"
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert_eq!(req.extra.get("reasoning_effort"), Some(&json!("high")));
        assert_eq!(req.extra.get("max_completion_tokens"), Some(&json!(1024)));
        assert!(!req.extra.contains_key("model"));
        assert!(!req.extra.contains_key("messages"));
        assert!(!req.extra.contains_key("target_provider"));
        assert!(!req.extra.contains_key("provider"));
    }

    #[test]
    fn openai_chat_gateway_fields_collects_underscore_prefixed_keys() {
        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hello"}],
                "_session_context": {"epoch": 3},
                "reasoning_effort": "high"
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert_eq!(
            req.gateway_fields.get("_session_context"),
            Some(&json!({"epoch": 3}))
        );
        assert!(!req.extra.contains_key("_session_context"));
        assert_eq!(req.extra.get("reasoning_effort"), Some(&json!("high")));
    }

    #[test]
    fn anthropic_chat_gateway_fields_collects_underscore_prefixed_keys() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "model": "claude-opus-4-6",
                "messages": [{"role": "user", "content": "hello"}],
                "_trace": {"id": "abc"},
                "thinking": {"type": "enabled", "budget_tokens": 1024}
            }),
            "claude-opus-4-6",
        )
        .expect("request");

        assert_eq!(
            req.gateway_fields.get("_trace"),
            Some(&json!({"id": "abc"}))
        );
        assert!(!req.extra.contains_key("_trace"));
        assert!(req.extra.contains_key("thinking"));
    }

    #[test]
    fn anthropic_requests_default_to_streaming() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}],
                "top_k": 5,
                "stop_sequences": ["DONE", "HALT"]
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert!(req.stream);
        assert_eq!(req.top_k, Some(5));
        assert_eq!(req.stop_sequences, Some(json!(["DONE", "HALT"])));
        assert_eq!(
            req.metadata
                .get(ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY)
                .map(String::as_str),
            Some("claude-3-5-sonnet-latest")
        );
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
    fn anthropic_chat_extra_preserves_unknown_fields_only() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "model": "claude-opus-4-6",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 1400,
                "thinking": {
                    "type": "enabled",
                    "budget_tokens": 1024,
                    "display": "omitted"
                },
                "output_config": {
                    "effort": "medium"
                },
                "metadata": {
                    "trace_id": "abc"
                }
            }),
            "claude-opus-4-6",
        )
        .expect("request");

        assert_eq!(req.max_tokens, Some(1400));
        assert_eq!(
            req.extra.get("thinking"),
            Some(&json!({
                "type": "enabled",
                "budget_tokens": 1024,
                "display": "omitted"
            }))
        );
        assert_eq!(
            req.extra.get("output_config"),
            Some(&json!({"effort": "medium"}))
        );
        assert_eq!(req.extra.get("metadata"), Some(&json!({"trace_id": "abc"})));
        assert!(!req.extra.contains_key("messages"));
        assert!(!req.extra.contains_key("max_tokens"));
    }

    #[test]
    fn responses_request_maps_reasoning_effort_to_reasoning() {
        let req = openai_payload_to_responses_request(
            &json!({
                "model": "gpt-5.5",
                "input": [{"role": "user", "content": "hello"}],
                "tools": [{"type": "function", "name": "lookup"}],
                "reasoning_effort": "low"
            }),
            "gpt-5.5",
        )
        .expect("request");

        assert_eq!(req.reasoning, Some(json!({"effort": "low"})));
        assert!(!req.extra.contains_key("reasoning_effort"));
    }

    #[test]
    fn responses_request_preserves_function_call_output_input_items() {
        let req = openai_payload_to_responses_request(
            &json!({
                "model": "gpt-5.5",
                "input": [
                    {"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{}"},
                    {"type": "function_call_output", "call_id": "call_1", "output": "sunny"}
                ]
            }),
            "gpt-5.5",
        )
        .expect("request");

        let input = req.input.expect("input");
        let items = input.as_array().expect("input array");
        assert_eq!(
            items[0].get("type").and_then(Value::as_str),
            Some("function_call")
        );
        assert_eq!(
            items[1].get("type").and_then(Value::as_str),
            Some("function_call_output")
        );
    }

    #[test]
    fn responses_request_strips_thinking_budget() {
        let req = openai_payload_to_responses_request(
            &json!({
                "model": "gpt-5.5",
                "input": [{"role": "user", "content": "hello"}],
                "reasoning": {"effort": "high"},
                "thinking_budget": 8192
            }),
            "gpt-5.5",
        )
        .expect("request");

        assert_eq!(req.reasoning, Some(json!({"effort": "high"})));
        assert!(!req.extra.contains_key("thinking_budget"));
    }

    #[test]
    fn chat_request_preserves_thinking_budget_in_extra() {
        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-5.5",
                "messages": [{"role": "user", "content": "hello"}],
                "thinking_budget": 8192
            }),
            "gpt-5.5",
        )
        .expect("request");

        assert_eq!(req.extra.get("thinking_budget"), Some(&json!(8192)));
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

        assert_eq!(filtered.reasoning, Some(json!({"effort": "high"})));
        assert!(!filtered.extra.contains_key("reasoning"));
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

    #[test]
    fn openai_request_preserves_raw_messages_for_tool_loop() {
        let tool_call = json!({
            "id": "call_123",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": "{\"location\": \"San Francisco\"}"
            }
        });

        let messages = json!([
            {
                "role": "user",
                "content": "What's the weather in San Francisco?"
            },
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [tool_call]
            },
            {
                "role": "tool",
                "tool_call_id": "call_123",
                "content": "The weather is sunny and 75°F"
            }
        ]);

        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-5.5",
                "messages": messages
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        // Verify raw_messages are preserved
        assert!(req.raw_messages.is_some());
        let raw = req.raw_messages.as_ref().expect("raw messages");
        assert!(raw.is_array());
        let raw_array = raw.as_array().unwrap();
        assert_eq!(raw_array.len(), 3);

        // Verify metadata marks this as OpenAI format
        assert!(req.has_openai_raw_messages());

        // Verify the assistant message has tool_calls preserved
        let assistant_msg = &raw_array[1];
        assert_eq!(
            assistant_msg.get("role").and_then(Value::as_str),
            Some("assistant")
        );
        assert!(assistant_msg.get("tool_calls").is_some());

        // Verify the tool message has tool_call_id preserved
        let tool_msg = &raw_array[2];
        assert_eq!(tool_msg.get("role").and_then(Value::as_str), Some("tool"));
        assert_eq!(
            tool_msg.get("tool_call_id").and_then(Value::as_str),
            Some("call_123")
        );
    }

    #[test]
    fn openai_chat_request_marks_client_protocol() {
        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-4o-mini",
                "messages": [{"role": "user", "content": "hello"}]
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert_eq!(req.client_protocol(), Some(ClientProtocol::OpenAiChat));
    }

    #[test]
    fn openai_chat_request_marks_placeholder_signature_status_for_reasoning() {
        let req = openai_payload_to_chat_request(
            &json!({
                "model": "gpt-4o-mini",
                "messages": [{
                    "role": "assistant",
                    "content": "answer",
                    "reasoning_content": "reasoning"
                }]
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert_eq!(
            req.thinking_signature_status(),
            Some(ThinkingSignatureStatus::Placeholder)
        );
    }

    #[test]
    fn anthropic_chat_request_marks_client_protocol() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "model": "claude-3-5-sonnet-latest",
                "messages": [{"role": "user", "content": "hello"}]
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert_eq!(
            req.client_protocol(),
            Some(ClientProtocol::AnthropicMessages)
        );
    }

    #[test]
    fn anthropic_chat_request_marks_verbatim_signature_status() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "model": "claude-3-5-sonnet-latest",
                "messages": [{
                    "role": "assistant",
                    "content": [{
                        "type": "thinking",
                        "thinking": "original reasoning",
                        "signature": "real-signature"
                    }]
                }]
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert_eq!(
            req.thinking_signature_status(),
            Some(ThinkingSignatureStatus::Verbatim)
        );
    }

    #[test]
    fn anthropic_chat_request_marks_placeholder_signature_status() {
        let req = anthropic_payload_to_chat_request(
            &json!({
                "model": "claude-3-5-sonnet-latest",
                "messages": [{
                    "role": "assistant",
                    "content": [{
                        "type": "thinking",
                        "thinking": "renderer-only reasoning",
                        "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE
                    }]
                }]
            }),
            "claude-3-5-sonnet-latest",
        )
        .expect("request");

        assert_eq!(
            req.thinking_signature_status(),
            Some(ThinkingSignatureStatus::Placeholder)
        );
    }
}
