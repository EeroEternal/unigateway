use std::collections::HashMap;

use serde_json::{Value, json};

use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::openai_messages_to_anthropic_messages;
use crate::request::{
    ClientProtocol, MessageRole, ProxyChatRequest, content_blocks_to_anthropic_request,
    openai_tool_choice_to_anthropic_tool_choice, openai_tools_to_anthropic_tools,
    validate_anthropic_request_messages,
};
use crate::transport::TransportRequest;

pub fn build_chat_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyChatRequest,
) -> Result<TransportRequest, GatewayError> {
    let mut system_parts = Vec::new();
    let mut fallback_messages = Vec::new();
    for message in &request.messages {
        match message.role {
            MessageRole::System => {
                let text = message.text_content();
                if !text.is_empty() {
                    system_parts.push(text);
                }
            }
            MessageRole::User | MessageRole::Assistant => {
                fallback_messages.push(json!({
                    "role": anthropic_role(message.role),
                    "content": Value::Array(content_blocks_to_anthropic_request(&message.content)?),
                }));
            }
            MessageRole::Tool => {}
        }
    }

    let fallback_system = request.system.clone().or_else(|| {
        if system_parts.is_empty() {
            None
        } else {
            Some(Value::String(system_parts.join("\n")))
        }
    });
    let (system, messages) = anthropic_chat_messages(request, fallback_system, fallback_messages)?;

    let mut payload = serde_json::Map::from_iter([
        (
            "model".to_string(),
            Value::String(resolved_model(endpoint, &request.model)),
        ),
        ("messages".to_string(), messages),
        (
            "max_tokens".to_string(),
            json!(request.max_tokens.unwrap_or(1024)),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ]);

    if let Some(system) = system {
        payload.insert("system".to_string(), system);
    }
    if let Some(temperature) = request.temperature {
        payload.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = request.top_p {
        payload.insert("top_p".to_string(), json!(top_p));
    }
    if let Some(top_k) = request.top_k {
        payload.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(stop_sequences) = request.stop_sequences.clone() {
        payload.insert("stop_sequences".to_string(), stop_sequences);
    }
    if let Some(tools) = anthropic_tools(request)? {
        payload.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = anthropic_tool_choice(request)? {
        payload.insert("tool_choice".to_string(), tool_choice);
    }
    for (key, value) in request.extra.clone() {
        payload.entry(key).or_insert(value);
    }

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "messages"),
        anthropic_headers(endpoint),
        &Value::Object(payload),
        None,
    )
}

fn anthropic_chat_messages(
    request: &ProxyChatRequest,
    fallback_system: Option<Value>,
    fallback_messages: Vec<Value>,
) -> Result<(Option<Value>, Value), GatewayError> {
    let Some(raw_messages) = request.raw_messages.as_ref() else {
        return Ok((fallback_system, Value::Array(fallback_messages)));
    };

    if is_openai_chat_request(request) {
        return openai_messages_to_anthropic_messages(raw_messages, fallback_system);
    }

    validate_anthropic_request_messages(raw_messages)?;
    Ok((fallback_system, raw_messages.clone()))
}

fn anthropic_tools(request: &ProxyChatRequest) -> Result<Option<Value>, GatewayError> {
    if is_openai_chat_request(request) {
        return openai_tools_to_anthropic_tools(request.tools.clone());
    }

    Ok(request.tools.clone())
}

fn anthropic_tool_choice(request: &ProxyChatRequest) -> Result<Option<Value>, GatewayError> {
    if !is_openai_chat_request(request) {
        return Ok(request.tool_choice.clone());
    }

    openai_tool_choice_to_anthropic_tool_choice(request.tool_choice.clone())
}

fn is_openai_chat_request(request: &ProxyChatRequest) -> bool {
    request.client_protocol() == Some(ClientProtocol::OpenAiChat)
        || request.has_openai_raw_messages()
}

fn anthropic_role(role: MessageRole) -> &'static str {
    match role {
        MessageRole::Assistant => "assistant",
        _ => "user",
    }
}

fn anthropic_headers(endpoint: &DriverEndpointContext) -> HashMap<String, String> {
    HashMap::from([
        (
            "x-api-key".to_string(),
            endpoint.api_key.expose_secret().to_string(),
        ),
        ("anthropic-version".to_string(), "2023-06-01".to_string()),
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

fn join_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path)
}
