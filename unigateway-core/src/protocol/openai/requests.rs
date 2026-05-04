use serde_json::{Value, json};

use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::{
    MessageRole, OPENAI_RAW_MESSAGES_KEY, ProxyChatRequest, ProxyEmbeddingsRequest,
    ProxyResponsesRequest, anthropic_messages_to_openai_messages,
    anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
};
use crate::transport::TransportRequest;
use std::collections::HashMap;

pub fn build_chat_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyChatRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        (
            "messages".to_string(),
            Value::Array(openai_chat_messages(request)?),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ]);

    if let Some(temperature) = request.temperature {
        payload.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        payload.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = request.top_k {
        payload.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(max_tokens) = request.max_tokens {
        payload.insert("max_tokens".to_string(), json!(max_tokens));
    }
    if let Some(stop) = request.stop_sequences.clone() {
        payload.insert("stop".to_string(), stop);
    }
    if let Some(tools) = anthropic_tools_to_openai_tools(request.tools.clone()) {
        payload.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) =
        anthropic_tool_choice_to_openai_tool_choice(request.tool_choice.clone())?
    {
        payload.insert("tool_choice".to_string(), tool_choice);
    }
    for (key, value) in request.extra.clone() {
        payload.entry(key).or_insert(value);
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "chat/completions"),
        openai_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

fn openai_chat_messages(request: &ProxyChatRequest) -> Result<Vec<Value>, GatewayError> {
    if let Some(raw_messages) = request.raw_messages.as_ref() {
        // Check if raw_messages are in OpenAI format (preserved from client)
        if request.metadata.contains_key(OPENAI_RAW_MESSAGES_KEY) {
            if let Some(messages_array) = raw_messages.as_array() {
                return Ok(messages_array.clone());
            }
            return Err(GatewayError::InvalidRequest(
                "openai raw_messages must be an array".to_string(),
            ));
        }
        // Otherwise, treat as Anthropic format and convert
        return anthropic_messages_to_openai_messages(raw_messages);
    }

    Ok(request
        .messages
        .iter()
        .map(|message| {
            json!({
                "role": openai_role(message.role),
                "content": message.content,
            })
        })
        .collect())
}
pub fn build_responses_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyResponsesRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ]);

    if let Some(input) = request.input.clone() {
        payload.insert("input".to_string(), input);
    }
    if let Some(instructions) = request.instructions.clone() {
        payload.insert("instructions".to_string(), Value::String(instructions));
    }
    if let Some(temperature) = request.temperature {
        payload.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        payload.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(max_output_tokens) = request.max_output_tokens {
        payload.insert("max_output_tokens".to_string(), json!(max_output_tokens));
    }
    if let Some(tools) = request.tools.clone() {
        payload.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = request.tool_choice.clone() {
        payload.insert("tool_choice".to_string(), tool_choice);
    }
    if let Some(previous_response_id) = request.previous_response_id.clone() {
        payload.insert(
            "previous_response_id".to_string(),
            Value::String(previous_response_id),
        );
    }
    if let Some(request_metadata) = request.request_metadata.clone() {
        payload.insert("metadata".to_string(), request_metadata);
    }
    for (key, value) in request.extra.clone() {
        payload.entry(key).or_insert(value);
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "responses"),
        openai_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

pub fn build_embeddings_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyEmbeddingsRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        ("input".to_string(), json!(request.input)),
    ]);

    if let Some(encoding_format) = request.encoding_format.clone() {
        payload.insert(
            "encoding_format".to_string(),
            Value::String(encoding_format),
        );
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "embeddings"),
        openai_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

fn openai_headers(endpoint: &DriverEndpointContext) -> HashMap<String, String> {
    HashMap::from([
        (
            "authorization".to_string(),
            format!("Bearer {}", endpoint.api_key.expose_secret()),
        ),
        ("content-type".to_string(), "application/json".to_string()),
    ])
}

fn resolved_model(endpoint: &DriverEndpointContext, requested_model: &str) -> String {
    endpoint
        .model_policy
        .model_mapping
        .get(requested_model)
        .cloned()
        .or_else(|| endpoint.model_policy.default_model.clone())
        .unwrap_or_else(|| requested_model.to_string())
}

fn openai_role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn join_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Message, MessageRole, ProxyChatRequest};
    use std::collections::HashMap;

    #[test]
    fn openai_chat_messages_preserves_tool_call_structure() {
        let tool_call = json!({
            "id": "call_123",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": "{\"location\": \"San Francisco\"}"
            }
        });

        let raw_messages = json!([
            {
                "role": "user",
                "content": "What's the weather?"
            },
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [tool_call]
            },
            {
                "role": "tool",
                "tool_call_id": "call_123",
                "content": "Sunny and 75°F"
            }
        ]);

        let mut metadata = HashMap::new();
        metadata.insert(OPENAI_RAW_MESSAGES_KEY.to_string(), "true".to_string());

        let request = ProxyChatRequest {
            model: "gpt-5.5".to_string(),
            messages: vec![
                Message {
                    role: MessageRole::User,
                    content: "What's the weather?".to_string(),
                },
                Message {
                    role: MessageRole::Assistant,
                    content: String::new(),
                },
                Message {
                    role: MessageRole::Tool,
                    content: "Sunny and 75°F".to_string(),
                },
            ],
            raw_messages: Some(raw_messages),
            metadata,
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            extra: HashMap::new(),
        };

        let messages = openai_chat_messages(&request).expect("messages");
        assert_eq!(messages.len(), 3);

        // Verify tool_calls are preserved
        let assistant_msg = &messages[1];
        assert_eq!(
            assistant_msg.get("role").and_then(Value::as_str),
            Some("assistant")
        );
        assert!(assistant_msg.get("tool_calls").is_some());

        // Verify tool_call_id is preserved
        let tool_msg = &messages[2];
        assert_eq!(tool_msg.get("role").and_then(Value::as_str), Some("tool"));
        assert_eq!(
            tool_msg.get("tool_call_id").and_then(Value::as_str),
            Some("call_123")
        );
    }

    #[test]
    fn openai_chat_messages_falls_back_to_flattened_when_no_raw() {
        let request = ProxyChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message {
                role: MessageRole::User,
                content: "Hello".to_string(),
            }],
            raw_messages: None,
            metadata: HashMap::new(),
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            extra: HashMap::new(),
        };

        let messages = openai_chat_messages(&request).expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].get("role").and_then(Value::as_str),
            Some("user")
        );
        assert_eq!(
            messages[0].get("content").and_then(Value::as_str),
            Some("Hello")
        );
    }
}
