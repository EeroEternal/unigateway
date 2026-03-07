use anyhow::{anyhow, Context, Result};
use llm_connector::{
    types::{ChatRequest, Message, Role},
    ChatResponse, LlmClient,
};
use serde_json::{json, Value};

pub enum UpstreamProtocol {
    OpenAi,
    Anthropic,
}

pub fn openai_payload_to_chat_request(payload: &Value, default_model: &str) -> Result<ChatRequest> {
    let mut req = ChatRequest::new(
        payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(default_model)
            .to_string(),
    );

    req.messages = openai_messages(payload)?;
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
    req.stream = payload.get("stream").and_then(Value::as_bool);

    Ok(req)
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
    messages.extend(anthropic_messages(payload)?);
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
    req.stream = payload.get("stream").and_then(Value::as_bool);

    Ok(req)
}

pub async fn invoke_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
) -> Result<ChatResponse> {
    if api_key.is_empty() {
        return Err(anyhow!("missing upstream api key"));
    }

    let client = match protocol {
        UpstreamProtocol::OpenAi => LlmClient::openai(api_key, base_url)
            .context("failed to create openai client")?,
        UpstreamProtocol::Anthropic => {
            LlmClient::anthropic_with_config(api_key, base_url, None, None)
                .context("failed to create anthropic client")?
        }
    };

    client.chat(req).await.context("llm-connector chat failed")
}

pub fn chat_response_to_openai_json(resp: &ChatResponse) -> Value {
    let content = resp
        .choices
        .first()
        .map(|c| c.message.content_as_text())
        .unwrap_or_default();

    json!({
        "id": resp.id,
        "object": if resp.object.is_empty() { "chat.completion" } else { &resp.object },
        "created": resp.created,
        "model": resp.model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": resp.finish_reason().unwrap_or("stop")
            }
        ],
        "usage": resp.usage.as_ref().map(|u| json!({
            "prompt_tokens": u.prompt_tokens,
            "completion_tokens": u.completion_tokens,
            "total_tokens": u.total_tokens
        }))
    })
}

pub fn chat_response_to_anthropic_json(resp: &ChatResponse) -> Value {
    let content = resp
        .choices
        .first()
        .map(|c| c.message.content_as_text())
        .unwrap_or_default();

    json!({
        "id": resp.id,
        "type": "message",
        "role": "assistant",
        "model": resp.model,
        "content": [
            {
                "type": "text",
                "text": content
            }
        ],
        "stop_reason": resp.finish_reason().unwrap_or("end_turn"),
        "usage": {
            "input_tokens": resp.prompt_tokens(),
            "output_tokens": resp.completion_tokens()
        }
    })
}

fn openai_messages(payload: &Value) -> Result<Vec<Message>> {
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

fn anthropic_messages(payload: &Value) -> Result<Vec<Message>> {
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
