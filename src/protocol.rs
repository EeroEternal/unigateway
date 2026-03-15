use anyhow::{Context, Result, anyhow};
use llm_connector::{
    ChatResponse, LlmClient,
    types::{ChatRequest, EmbedRequest, EmbedResponse, Message, Role},
};
use serde_json::{Value, json};
use tracing::debug;

pub enum UpstreamProtocol {
    OpenAi,
    Anthropic,
}

fn stream_flag(payload: &Value, default: bool) -> Option<bool> {
    Some(
        payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(default),
    )
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
    req.stream = stream_flag(payload, true);

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
    req.stream = stream_flag(payload, true);

    Ok(req)
}

/// Build OpenAI-compatible client. When family_id is Some("minimax"), uses llm_providers-derived
/// base_url with MiniMax chat path (/v1/text/chatcompletion_v2) via ConfigurableProtocol.
pub async fn invoke_with_connector(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<ChatResponse> {
    debug!(
        protocol = match protocol {
            UpstreamProtocol::OpenAi => "openai",
            UpstreamProtocol::Anthropic => "anthropic",
        },
        base_url,
        family_id = family_id.unwrap_or(""),
        model = req.model.as_str(),
        stream = req.stream.unwrap_or(false),
        message_count = req.messages.len(),
        "invoking llm-connector"
    );
    let client = match protocol {
        UpstreamProtocol::OpenAi => build_openai_client(base_url, api_key, family_id)?,
        UpstreamProtocol::Anthropic => {
            if api_key.is_empty() {
                return Err(anyhow!("missing upstream api key"));
            }
            LlmClient::anthropic_with_config(api_key, base_url, None, None)
                .context("failed to create anthropic client")?
        }
    };
    let resp = client
        .chat(req)
        .await
        .context("llm-connector chat failed")?;
    debug!(
        response_id = resp.id.as_str(),
        response_model = resp.model.as_str(),
        response_created = resp.created,
        choices = resp.choices.len(),
        usage_present = resp.usage.is_some(),
        first_content_len = resp
            .choices
            .first()
            .map(|c| c.message.content_as_text().len())
            .unwrap_or(0),
        "llm-connector chat returned"
    );
    Ok(resp)
}

/// Build the same client as invoke_with_connector (OpenAI path only). Used for streaming.
/// MiniMax supports the standard OpenAI-compatible /v1/chat/completions endpoint,
/// so all providers use the same standard OpenAI client.
fn build_openai_client(
    base_url: &str,
    api_key: &str,
    _family_id: Option<&str>,
) -> Result<LlmClient, anyhow::Error> {
    if api_key.is_empty() {
        return Err(anyhow!("missing upstream api key"));
    }
    LlmClient::openai(api_key, base_url).context("failed to create openai client")
}

/// Streaming chat for supported protocols. Returns a unified llm-connector stream.
pub async fn invoke_with_connector_stream(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    req: &ChatRequest,
    family_id: Option<&str>,
) -> Result<llm_connector::types::ChatStream, anyhow::Error> {
    let client = match protocol {
        UpstreamProtocol::OpenAi => build_openai_client(base_url, api_key, family_id)?,
        UpstreamProtocol::Anthropic => {
            if api_key.is_empty() {
                return Err(anyhow!("missing upstream api key"));
            }
            LlmClient::anthropic_with_config(api_key, base_url, None, None)
                .context("failed to create anthropic client")?
        }
    };
    client
        .chat_stream(req)
        .await
        .context("llm-connector chat_stream failed")
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

// --- Embeddings ---

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

pub async fn invoke_embeddings(
    base_url: &str,
    api_key: &str,
    req: &EmbedRequest,
) -> Result<EmbedResponse> {
    debug!(
        base_url,
        model = req.model.as_str(),
        input_count = req.input.len(),
        "invoking llm-connector embed"
    );
    let client = build_openai_client(base_url, api_key, None)?;
    let resp = client
        .embed(req)
        .await
        .context("llm-connector embed failed")?;
    debug!(
        model = resp.model.as_str(),
        data_count = resp.data.len(),
        "llm-connector embed returned"
    );
    Ok(resp)
}

pub fn embed_response_to_openai_json(resp: &EmbedResponse) -> Value {
    let data: Vec<Value> = resp
        .data
        .iter()
        .map(|d| {
            json!({
                "object": "embedding",
                "embedding": d.embedding,
                "index": d.index,
            })
        })
        .collect();

    json!({
        "object": "list",
        "data": data,
        "model": resp.model,
        "usage": {
            "prompt_tokens": resp.usage.prompt_tokens,
            "total_tokens": resp.usage.total_tokens
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{anthropic_payload_to_chat_request, openai_payload_to_chat_request};
    use serde_json::json;

    #[test]
    fn openai_requests_default_to_streaming() {
        let req = openai_payload_to_chat_request(
            &json!({
                "messages": [{"role": "user", "content": "hello"}]
            }),
            "gpt-4o-mini",
        )
        .expect("request");

        assert_eq!(req.stream, Some(true));
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
}
