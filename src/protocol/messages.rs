use anyhow::{Result, anyhow};
use llm_connector::types::{Message, Role};
use serde_json::Value;

pub(super) fn stream_flag(payload: &Value, default: bool) -> Option<bool> {
    Some(
        payload
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(default),
    )
}

pub(super) fn chat_messages(payload: &Value) -> Result<Vec<Message>> {
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

pub(super) fn extract_text_content(value: &Value) -> String {
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
