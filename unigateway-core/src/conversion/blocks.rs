use serde_json::{Value, json};

use crate::error::GatewayError;
use crate::request::{ContentBlock, THINKING_SIGNATURE_PLACEHOLDER_VALUE};

impl ContentBlock {
    pub fn to_anthropic_value(&self) -> Value {
        match self {
            Self::Text { text } => json!({
                "type": "text",
                "text": text,
            }),
            Self::Thinking {
                thinking,
                signature,
            } => {
                let mut block = serde_json::Map::from_iter([
                    ("type".to_string(), Value::String("thinking".to_string())),
                    ("thinking".to_string(), Value::String(thinking.clone())),
                ]);
                if let Some(signature) = signature {
                    block.insert("signature".to_string(), Value::String(signature.clone()));
                }
                Value::Object(block)
            }
            Self::ToolUse { id, name, input } => json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }),
            Self::ToolResult {
                tool_use_id,
                content,
            } => json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
            }),
        }
    }

    pub fn to_anthropic_request_value(&self) -> Result<Value, GatewayError> {
        if let Self::Thinking {
            signature: Some(signature),
            ..
        } = self
            && is_placeholder_thinking_signature(signature)
        {
            return Err(GatewayError::InvalidRequest(
                "placeholder thinking signature cannot be sent to anthropic upstream".to_string(),
            ));
        }

        Ok(self.to_anthropic_value())
    }

    pub(crate) fn to_openai_tool_call(&self) -> Result<Option<Value>, GatewayError> {
        let Self::ToolUse { id, name, input } = self else {
            return Ok(None);
        };
        let arguments = serde_json::to_string(input).map_err(|error| {
            GatewayError::InvalidRequest(format!(
                "failed to serialize anthropic tool_use input: {error}",
            ))
        })?;

        Ok(Some(json!({
            "id": id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments,
            }
        })))
    }

    pub(crate) fn to_openai_tool_message(&self) -> Option<Value> {
        let Self::ToolResult {
            tool_use_id,
            content,
        } = self
        else {
            return None;
        };

        Some(json!({
            "role": "tool",
            "tool_call_id": tool_use_id,
            "content": content,
        }))
    }
}

pub fn content_blocks_to_anthropic(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(ContentBlock::to_anthropic_value)
        .collect()
}

pub fn content_blocks_to_anthropic_request(
    blocks: &[ContentBlock],
) -> Result<Vec<Value>, GatewayError> {
    blocks
        .iter()
        .map(ContentBlock::to_anthropic_request_value)
        .collect()
}

pub fn openai_message_to_content_blocks(
    message: &Value,
) -> Result<Vec<ContentBlock>, GatewayError> {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if role == "tool" {
        let tool_use_id = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "openai tool message requires tool_call_id".to_string(),
                )
            })?;
        return Ok(vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: json_content_to_string(message.get("content")),
        }]);
    }

    let mut blocks = Vec::new();

    if let Some(thinking) = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"))
        .and_then(Value::as_str)
    {
        blocks.push(ContentBlock::Thinking {
            thinking: thinking.to_string(),
            signature: None,
        });
    }

    blocks.extend(openai_content_to_blocks(message.get("content"))?);

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            blocks.push(openai_tool_call_to_content_block(tool_call)?);
        }
    }

    Ok(blocks)
}

pub fn anthropic_content_to_blocks(content: &Value) -> Result<Vec<ContentBlock>, GatewayError> {
    match content {
        Value::String(text) => Ok(vec![ContentBlock::Text { text: text.clone() }]),
        Value::Array(items) => items.iter().map(anthropic_block_to_content_block).collect(),
        Value::Null => Ok(Vec::new()),
        other => Err(GatewayError::InvalidRequest(format!(
            "unsupported anthropic content value: {other}",
        ))),
    }
}

pub fn openai_message_to_anthropic_content_blocks(message: &Value) -> Vec<Value> {
    let mut content_blocks = Vec::new();

    if let Some(thinking) = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"))
        .and_then(Value::as_str)
    {
        content_blocks.push(json!({
            "type": "thinking",
            "thinking": thinking,
            "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE,
        }));
    }

    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            content_blocks.push(json!({
                "type": "text",
                "text": text,
            }));
        }
        Some(Value::Array(blocks)) => {
            content_blocks.extend(blocks.iter().filter_map(|block| {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    Some(block.clone())
                } else {
                    None
                }
            }));
        }
        _ => {}
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        content_blocks.extend(
            tool_calls
                .iter()
                .filter_map(openai_tool_call_to_anthropic_block),
        );
    }

    content_blocks
}

pub fn is_placeholder_thinking_signature(signature: &str) -> bool {
    signature == THINKING_SIGNATURE_PLACEHOLDER_VALUE
}

pub(crate) fn anthropic_block_to_content_block(
    block: &Value,
) -> Result<ContentBlock, GatewayError> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => Ok(ContentBlock::Text {
            text: block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        Some("thinking") => Ok(ContentBlock::Thinking {
            thinking: block
                .get("thinking")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            signature: block
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        Some("tool_use") => Ok(ContentBlock::ToolUse {
            id: block
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest("anthropic tool_use requires id".to_string())
                })?
                .to_string(),
            name: block
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest("anthropic tool_use requires name".to_string())
                })?
                .to_string(),
            input: block.get("input").cloned().unwrap_or_else(|| json!({})),
        }),
        Some("tool_result") => Ok(ContentBlock::ToolResult {
            tool_use_id: block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "anthropic tool_result requires tool_use_id".to_string(),
                    )
                })?
                .to_string(),
            content: anthropic_tool_result_content_to_string(block.get("content")),
        }),
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "unsupported anthropic content block type: {other}",
        ))),
        None => Err(GatewayError::InvalidRequest(
            "anthropic content block is missing type".to_string(),
        )),
    }
}

pub(crate) fn anthropic_blocks(content: Value) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({ "type": "text", "text": text })],
        Value::Array(blocks) => blocks,
        _ => Vec::new(),
    }
}

fn openai_content_to_blocks(content: Option<&Value>) -> Result<Vec<ContentBlock>, GatewayError> {
    match content {
        Some(Value::String(text)) if text.is_empty() => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![ContentBlock::Text { text: text.clone() }]),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|block| !is_empty_openai_text_block(block))
            .map(openai_content_block_to_content_block)
            .collect(),
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "unsupported openai message content value: {other}",
        ))),
    }
}

fn openai_content_block_to_content_block(block: &Value) -> Result<ContentBlock, GatewayError> {
    match block.get("type").and_then(Value::as_str) {
        Some("text" | "input_text") => Ok(ContentBlock::Text {
            text: block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "unsupported openai content block type: {other}",
        ))),
        None => Err(GatewayError::InvalidRequest(
            "openai content block is missing type".to_string(),
        )),
    }
}

fn is_empty_openai_text_block(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("text" | "input_text")
    ) && block.get("text").and_then(Value::as_str) == Some("")
}

fn openai_tool_call_to_content_block(tool_call: &Value) -> Result<ContentBlock, GatewayError> {
    let tool_id = tool_call
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| GatewayError::InvalidRequest("openai tool_call requires id".to_string()))?;
    let function = tool_call.get("function").ok_or_else(|| {
        GatewayError::InvalidRequest("openai tool_call requires function".to_string())
    })?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            GatewayError::InvalidRequest("openai tool_call.function requires name".to_string())
        })?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let input = serde_json::from_str(arguments).unwrap_or_else(|_| json!({ "_raw": arguments }));

    Ok(ContentBlock::ToolUse {
        id: tool_id.to_string(),
        name: name.to_string(),
        input,
    })
}

fn openai_tool_call_to_anthropic_block(tool_call: &Value) -> Option<Value> {
    let function = tool_call.get("function")?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let parsed_input = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}));

    Some(json!({
        "type": "tool_use",
        "id": tool_call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("toolu_unknown"),
        "name": function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("tool"),
        "input": parsed_input,
    }))
}

fn anthropic_tool_result_content_to_string(content: Option<&Value>) -> String {
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

fn json_content_to_string(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}
