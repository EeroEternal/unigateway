mod client;
mod messages;

use anyhow::{Result, anyhow};
use llm_connector::{
    ChatResponse,
    types::{ChatRequest, EmbedRequest, EmbedResponse, Message, ResponsesRequest, Role},
};
use serde_json::{Value, json};

pub(crate) use client::{
    invoke_embeddings, invoke_responses_stream_with_connector, invoke_responses_with_connector,
    invoke_with_connector, invoke_with_connector_stream,
};
use messages::{chat_messages, stream_flag};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}
