use anyhow::{Result, anyhow};
use llm_connector::types::{ChatRequest, EmbedRequest, Message, ResponsesRequest, Role};
use serde_json::Value;
use unigateway_core::{
    Message as CoreMessage, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest,
    ProxyResponsesRequest,
};

pub fn openai_payload_to_chat_request(payload: &Value, default_model: &str) -> Result<ChatRequest> {
    let mut req = ChatRequest::new(
        payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
    );

    req.messages = chat_messages(payload)?;
    req.temperature = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    req.top_p = payload
        .get("top_p")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    req.max_tokens = payload
        .get("max_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    req.stream = stream_flag(payload, false);

    Ok(req)
}

pub fn openai_payload_to_responses_request(
    payload: &Value,
    default_model: &str,
) -> Result<ResponsesRequest> {
    let mut normalized = payload.clone();
    if normalized.get("model").and_then(Value::as_str).is_none() {
        normalized["model"] = Value::String(default_model.to_string());
    }

    serde_json::from_value::<ResponsesRequest>(normalized)
        .map_err(|e| anyhow!("failed to parse responses request: {e}"))
}

pub fn anthropic_payload_to_chat_request(
    payload: &Value,
    default_model: &str,
) -> Result<ChatRequest> {
    let mut req = ChatRequest::new(
        payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
    );

    let mut messages = Vec::new();
    if let Some(system) = payload.get("system").and_then(Value::as_str) {
        messages.push(Message::text(Role::System, system));
    }
    messages.extend(chat_messages(payload)?);
    req.messages = messages;

    req.temperature = payload
        .get("temperature")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    req.top_p = payload
        .get("top_p")
        .and_then(Value::as_f64)
        .map(|v| v as f32);
    req.max_tokens = payload
        .get("max_tokens")
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    req.stream = stream_flag(payload, true);

    Ok(req)
}

pub fn openai_payload_to_embed_request(
    payload: &Value,
    default_model: &str,
) -> Result<EmbedRequest> {
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(default_model)
        .to_string();

    let input = match payload.get("input") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => return Err(anyhow!("input must be a string or array of strings")),
    };

    let mut req = EmbedRequest::new_batch(model, input);
    if let Some(fmt) = payload.get("encoding_format").and_then(Value::as_str) {
        req = req.with_encoding_format(fmt);
    }
    Ok(req)
}

pub fn to_core_chat_request(request: &ChatRequest) -> ProxyChatRequest {
    ProxyChatRequest {
        model: request.model.clone(),
        messages: request.messages.iter().map(to_core_message).collect(),
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_tokens,
        stream: request.stream.unwrap_or(false),
        metadata: std::collections::HashMap::new(),
    }
}

pub fn to_core_responses_request(request: &ResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        instructions: request.instructions.clone(),
        temperature: request.temperature,
        top_p: request.top_p,
        max_output_tokens: request.max_output_tokens,
        stream: request.stream.unwrap_or(false),
        tools: request.tools.clone(),
        tool_choice: request.tool_choice.clone(),
        previous_response_id: request.previous_response_id.clone(),
        request_metadata: request.metadata.clone(),
        extra: filtered_response_extra(request),
        metadata: std::collections::HashMap::new(),
    }
}

pub fn to_core_embeddings_request(request: &EmbedRequest) -> ProxyEmbeddingsRequest {
    ProxyEmbeddingsRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        metadata: std::collections::HashMap::new(),
    }
}

fn stream_flag(payload: &Value, default: bool) -> Option<bool> {
    Some(
        payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(default),
    )
}

fn chat_messages(payload: &Value) -> Result<Vec<Message>> {
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
            Ok(Message::text(role, content))
        })
        .collect()
}

fn parse_role(role: &str) -> Role {
    match role {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
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

fn to_core_message(message: &Message) -> CoreMessage {
    CoreMessage {
        role: match message.role {
            Role::System => MessageRole::System,
            Role::Assistant => MessageRole::Assistant,
            Role::Tool => MessageRole::Tool,
            _ => MessageRole::User,
        },
        content: message.content_as_text(),
    }
}

fn filtered_response_extra(
    request: &ResponsesRequest,
) -> std::collections::HashMap<String, serde_json::Value> {
    request
        .extra
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        anthropic_payload_to_chat_request, openai_payload_to_chat_request,
        to_core_responses_request,
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

        assert_eq!(req.stream, Some(false));
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

        assert_eq!(req.stream, Some(false));
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

        assert_eq!(req.stream, Some(true));
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

        assert_eq!(req.stream, Some(false));
    }

    #[test]
    fn responses_extra_filter_strips_gateway_routing_hints_only() {
        let filtered = to_core_responses_request(&llm_connector::types::ResponsesRequest {
            model: "gpt-4.1-mini".to_string(),
            extra: std::collections::HashMap::from([
                (
                    "reasoning".to_string(),
                    serde_json::json!({"effort": "high"}),
                ),
                ("target_provider".to_string(), serde_json::json!("deepseek")),
                ("provider".to_string(), serde_json::json!("moonshot")),
            ]),
            ..llm_connector::types::ResponsesRequest::default()
        });

        assert!(filtered.extra.contains_key("reasoning"));
        assert!(!filtered.extra.contains_key("target_provider"));
        assert!(!filtered.extra.contains_key("provider"));
    }
}
