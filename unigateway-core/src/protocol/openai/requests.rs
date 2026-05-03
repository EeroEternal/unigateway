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
    if let Some(tools) = openai_tools(request.tools.clone()) {
        payload.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = openai_tool_choice(request.tool_choice.clone())? {
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

fn anthropic_messages_to_openai_messages(raw_messages: &Value) -> Result<Vec<Value>, GatewayError> {
    let Some(messages) = raw_messages.as_array() else {
        return Err(GatewayError::InvalidRequest(
            "anthropic messages must be an array".to_string(),
        ));
    };

    let mut openai_messages = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = message.get("content").cloned().unwrap_or(Value::Null);

        match role {
            "assistant" => {
                let mut content_blocks = Vec::new();
                let mut tool_calls = Vec::new();
                let mut thinking_parts = Vec::new();

                for block in anthropic_blocks(content) {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_use") => {
                            if let Some(tool_call) = anthropic_tool_use_to_openai_tool_call(&block)
                            {
                                tool_calls.push(tool_call);
                            }
                        }
                        Some("thinking") => {
                            if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                                thinking_parts.push(thinking.to_string());
                            }
                        }
                        _ => content_blocks.push(block),
                    }
                }

                let mut assistant_message = serde_json::Map::from_iter([
                    ("role".to_string(), Value::String("assistant".to_string())),
                    ("content".to_string(), Value::Array(content_blocks)),
                ]);
                if !tool_calls.is_empty() {
                    assistant_message.insert("tool_calls".to_string(), Value::Array(tool_calls));
                }
                if !thinking_parts.is_empty() {
                    assistant_message.insert(
                        "thinking".to_string(),
                        Value::String(thinking_parts.join("\n")),
                    );
                }
                openai_messages.push(Value::Object(assistant_message));
            }
            _ => {
                if content.is_string() {
                    openai_messages.push(json!({
                        "role": role,
                        "content": content,
                    }));
                    continue;
                }

                let mut user_blocks = Vec::new();
                for block in anthropic_blocks(content) {
                    if matches!(
                        block.get("type").and_then(Value::as_str),
                        Some("tool_result")
                    ) {
                        flush_user_blocks(&mut openai_messages, &mut user_blocks, role);
                        if let Some(tool_message) =
                            anthropic_tool_result_to_openai_tool_message(&block)
                        {
                            openai_messages.push(tool_message);
                        }
                    } else {
                        user_blocks.push(block);
                    }
                }
                flush_user_blocks(&mut openai_messages, &mut user_blocks, role);
            }
        }
    }

    Ok(openai_messages)
}

fn anthropic_blocks(content: Value) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({ "type": "text", "text": text })],
        Value::Array(blocks) => blocks,
        _ => Vec::new(),
    }
}

fn anthropic_tool_use_to_openai_tool_call(block: &Value) -> Option<Value> {
    let tool_id = block.get("id")?.as_str()?;
    let tool_name = block.get("name")?.as_str()?;
    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));

    Some(json!({
        "id": tool_id,
        "type": "function",
        "function": {
            "name": tool_name,
            "arguments": serde_json::to_string(&input).ok()?
        }
    }))
}

fn anthropic_tool_result_to_openai_tool_message(block: &Value) -> Option<Value> {
    let tool_use_id = block.get("tool_use_id")?.as_str()?;
    Some(json!({
        "role": "tool",
        "tool_call_id": tool_use_id,
        "content": anthropic_tool_result_content_to_openai_string(block.get("content"))
    }))
}

fn openai_tools(tools: Option<Value>) -> Option<Value> {
    let Value::Array(items) = tools? else {
        return None;
    };

    Some(Value::Array(
        items
            .into_iter()
            .map(|tool| {
                if tool.get("type").and_then(Value::as_str) == Some("function") {
                    return tool;
                }

                json!({
                    "type": "function",
                    "function": {
                        "name": tool.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        "description": tool.get("description").and_then(Value::as_str),
                        "parameters": tool
                            .get("input_schema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
                    }
                })
            })
            .collect(),
    ))
}

fn anthropic_tool_result_content_to_openai_string(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => {
            let text_parts = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .or_else(|| block.as_str())
                })
                .collect::<Vec<_>>();

            if text_parts.is_empty() {
                serde_json::to_string(&Value::Array(blocks.clone())).unwrap_or_default()
            } else {
                text_parts.join("\n")
            }
        }
        Some(Value::Null) | None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn flush_user_blocks(messages: &mut Vec<Value>, user_blocks: &mut Vec<Value>, role: &str) {
    if user_blocks.is_empty() {
        return;
    }

    messages.push(json!({
        "role": role,
        "content": Value::Array(std::mem::take(user_blocks)),
    }));
}

fn openai_tool_choice(tool_choice: Option<Value>) -> Result<Option<Value>, GatewayError> {
    match tool_choice {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" | "none" | "required" => Ok(Some(Value::String(mode))),
            "any" => Ok(Some(Value::String("required".to_string()))),
            other => Err(GatewayError::InvalidRequest(format!(
                "unsupported anthropic tool_choice mode: {other}",
            ))),
        },
        Some(Value::Object(obj)) => match obj.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(Value::String("auto".to_string()))),
            Some("any") => Ok(Some(Value::String("required".to_string()))),
            Some("none") => Ok(Some(Value::String("none".to_string()))),
            Some("tool") => obj
                .get("name")
                .and_then(Value::as_str)
                .map(|name| {
                    Value::Object(serde_json::Map::from_iter([
                        ("type".to_string(), Value::String("function".to_string())),
                        (
                            "function".to_string(),
                            Value::Object(serde_json::Map::from_iter([(
                                "name".to_string(),
                                Value::String(name.to_string()),
                            )])),
                        ),
                    ]))
                })
                .map(Some)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "anthropic tool_choice.type=tool requires a name".to_string(),
                    )
                }),
            Some("function") => Ok(Some(Value::Object(obj))),
            Some(other) => Err(GatewayError::InvalidRequest(format!(
                "unsupported anthropic tool_choice type: {other}",
            ))),
            None => Err(GatewayError::InvalidRequest(
                "anthropic tool_choice object is missing a type".to_string(),
            )),
        },
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "anthropic tool_choice must be a string or object, got: {other}",
        ))),
        None => Ok(None),
    }
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
