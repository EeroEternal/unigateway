use std::collections::HashMap;

use serde_json::{Value, json};

use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::{
    MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
use crate::transport::TransportRequest;

pub fn build_chat_request(
    endpoint: &DriverEndpointContext,
    request: &ProxyChatRequest,
) -> Result<TransportRequest, GatewayError> {
    let payload = json!({
        "model": resolved_model(endpoint, &request.model),
        "messages": request
            .messages
            .iter()
            .map(|message| json!({
                "role": openai_role(message.role),
                "content": message.content,
            }))
            .collect::<Vec<_>>(),
        "temperature": request.temperature,
        "top_p": request.top_p,
        "max_tokens": request.max_tokens,
        "stream": request.stream,
    });

    TransportRequest::post_json(
        Some(endpoint.endpoint_id.clone()),
        join_url(&endpoint.base_url, "chat/completions"),
        openai_headers(endpoint),
        &payload,
        None,
    )
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
