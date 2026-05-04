use serde_json::{Value, json};

use crate::error::GatewayError;
use crate::request::ContentBlock;

use super::blocks::{
    anthropic_block_to_content_block, anthropic_blocks, content_blocks_to_anthropic_request,
    is_placeholder_thinking_signature, openai_message_to_anthropic_request_content_blocks,
    openai_message_to_content_blocks,
};

pub fn openai_messages_to_anthropic_messages(
    raw_messages: &Value,
    fallback_system: Option<Value>,
) -> Result<(Option<Value>, Value), GatewayError> {
    let Some(messages) = raw_messages.as_array() else {
        return Err(GatewayError::InvalidRequest(
            "openai messages must be an array".to_string(),
        ));
    };

    let mut system_blocks = Vec::new();
    let mut anthropic_messages = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| GatewayError::InvalidRequest("message role is required".to_string()))?;

        match role {
            "system" => {
                system_blocks.extend(openai_system_content_blocks(message.get("content"))?);
            }
            "user" | "assistant" => {
                anthropic_messages.push(openai_chat_message_to_anthropic_message(message, role)?);
            }
            "tool" => {
                anthropic_messages.push(openai_tool_message_to_anthropic_message(message)?);
            }
            other => {
                return Err(GatewayError::InvalidRequest(format!(
                    "unsupported openai message role for anthropic request: {other}",
                )));
            }
        }
    }

    let system = if system_blocks.is_empty() {
        fallback_system
    } else {
        Some(collapse_openai_system_blocks(system_blocks))
    };

    Ok((system, Value::Array(anthropic_messages)))
}

pub fn anthropic_messages_to_openai_messages(
    raw_messages: &Value,
) -> Result<Vec<Value>, GatewayError> {
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
                            if let Some(tool_call) =
                                anthropic_block_to_content_block(&block)?.to_openai_tool_call()?
                            {
                                tool_calls.push(tool_call);
                            }
                        }
                        Some("thinking") => {
                            if let ContentBlock::Thinking { thinking, .. } =
                                anthropic_block_to_content_block(&block)?
                            {
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
                            anthropic_block_to_content_block(&block)?.to_openai_tool_message()
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

pub fn validate_anthropic_request_messages(messages: &Value) -> Result<(), GatewayError> {
    let Some(messages) = messages.as_array() else {
        return Err(GatewayError::InvalidRequest(
            "anthropic messages must be an array".to_string(),
        ));
    };

    for message in messages {
        if let Some(content) = message.get("content") {
            validate_anthropic_request_content(content)?;
        }
    }

    Ok(())
}

fn openai_chat_message_to_anthropic_message(
    message: &Value,
    role: &str,
) -> Result<Value, GatewayError> {
    let content = openai_message_to_anthropic_request_content_blocks(message)?;

    Ok(json!({
        "role": role,
        "content": Value::Array(content),
    }))
}

fn openai_tool_message_to_anthropic_message(message: &Value) -> Result<Value, GatewayError> {
    let blocks = openai_message_to_content_blocks(message)?;
    let content = content_blocks_to_anthropic_request(&blocks)?;

    Ok(json!({
        "role": "user",
        "content": Value::Array(content),
    }))
}

fn openai_system_content_blocks(content: Option<&Value>) -> Result<Vec<Value>, GatewayError> {
    match content {
        Some(Value::String(text)) => Ok(vec![json!({
            "type": "text",
            "text": text,
        })]),
        Some(Value::Array(blocks)) => {
            let parts = blocks
                .iter()
                .filter_map(|block| {
                    let block_type = block.get("type").and_then(Value::as_str);
                    let text = block.get("text").and_then(Value::as_str)?;

                    match block_type {
                        Some("text" | "input_text") => Some(json!({
                            "type": "text",
                            "text": text,
                        })),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            Ok(parts)
        }
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "unsupported openai system content for anthropic request: {other}",
        ))),
    }
}

fn collapse_openai_system_blocks(blocks: Vec<Value>) -> Value {
    match blocks.as_slice() {
        [block]
            if block.get("type").and_then(Value::as_str) == Some("text")
                && block.get("text").and_then(Value::as_str).is_some() =>
        {
            Value::String(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        }
        _ => Value::Array(blocks),
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

fn validate_anthropic_request_content(content: &Value) -> Result<(), GatewayError> {
    let Value::Array(blocks) = content else {
        return Ok(());
    };

    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("thinking")
            && block
                .get("signature")
                .and_then(Value::as_str)
                .is_some_and(is_placeholder_thinking_signature)
        {
            return Err(GatewayError::InvalidRequest(
                "placeholder thinking signature cannot be sent to anthropic upstream".to_string(),
            ));
        }
    }

    Ok(())
}
