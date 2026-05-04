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
            Self::Image { source, .. } => json!({
                "type": "image",
                "source": source,
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
            content: message
                .get("content")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
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
    openai_message_to_anthropic_content_blocks_with_signature(
        message,
        Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE),
        false,
    )
    .unwrap_or_default()
}

pub(crate) fn openai_message_to_anthropic_request_content_blocks(
    message: &Value,
) -> Result<Vec<Value>, GatewayError> {
    openai_message_to_anthropic_content_blocks_with_signature(message, None, true)
}

fn openai_message_to_anthropic_content_blocks_with_signature(
    message: &Value,
    thinking_signature: Option<&str>,
    strict: bool,
) -> Result<Vec<Value>, GatewayError> {
    let mut content_blocks = Vec::new();

    if let Some(thinking) = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"))
        .and_then(Value::as_str)
    {
        let mut block = serde_json::Map::from_iter([
            ("type".to_string(), Value::String("thinking".to_string())),
            ("thinking".to_string(), Value::String(thinking.to_string())),
        ]);
        if let Some(signature) = thinking_signature {
            block.insert(
                "signature".to_string(),
                Value::String(signature.to_string()),
            );
        }
        content_blocks.push(Value::Object(block));
    }

    match message.get("content") {
        Some(Value::String(text)) if !text.is_empty() => {
            content_blocks.push(json!({
                "type": "text",
                "text": text,
            }));
        }
        Some(Value::Array(blocks)) => {
            for block in blocks {
                if let Some(mapped_block) = openai_content_block_to_anthropic_block(block, strict)?
                {
                    content_blocks.push(mapped_block);
                }
            }
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

    Ok(content_blocks)
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
        Some("image") => Ok(ContentBlock::Image {
            source: block.get("source").cloned().ok_or_else(|| {
                GatewayError::InvalidRequest("anthropic image requires source".to_string())
            })?,
            detail: None,
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
            content: block
                .get("content")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new())),
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
        Some("image_url" | "input_image") => {
            let (source, detail) = openai_image_block_to_source(block)?;
            Ok(ContentBlock::Image { source, detail })
        }
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "unsupported openai content block type: {other}",
        ))),
        None => Err(GatewayError::InvalidRequest(
            "openai content block is missing type".to_string(),
        )),
    }
}

fn openai_content_block_to_anthropic_block(
    block: &Value,
    strict: bool,
) -> Result<Option<Value>, GatewayError> {
    match block.get("type").and_then(Value::as_str) {
        Some("text" | "input_text") => {
            let text = block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if text.is_empty() {
                return Ok(None);
            }

            Ok(Some(json!({
                "type": "text",
                "text": text,
            })))
        }
        Some("image_url" | "input_image") => match openai_image_block_to_source(block) {
            Ok((source, _)) => Ok(Some(json!({
                "type": "image",
                "source": source,
            }))),
            Err(error) if strict => Err(error),
            Err(_) => Ok(None),
        },
        Some(other) if strict => Err(GatewayError::InvalidRequest(format!(
            "unsupported openai content block type: {other}",
        ))),
        Some(_) => Ok(None),
        None if strict => Err(GatewayError::InvalidRequest(
            "openai content block is missing type".to_string(),
        )),
        None => Ok(None),
    }
}

fn openai_image_block_to_source(block: &Value) -> Result<(Value, Option<String>), GatewayError> {
    let detail = block
        .get("detail")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            block
                .get("image_url")
                .and_then(Value::as_object)
                .and_then(|image| image.get("detail"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    if let Some(file_id) = block.get("file_id").and_then(Value::as_str) {
        return Ok((
            json!({
                "type": "file",
                "file_id": file_id,
            }),
            detail,
        ));
    }

    let image_value = block.get("image_url").ok_or_else(|| {
        GatewayError::InvalidRequest("openai image block requires image_url".to_string())
    })?;

    let image_reference = match image_value {
        Value::String(url) => Some(url.as_str()),
        Value::Object(object) => object.get("url").and_then(Value::as_str),
        _ => None,
    }
    .ok_or_else(|| {
        GatewayError::InvalidRequest(
            "openai image block requires image_url.url or image_url string".to_string(),
        )
    })?;

    Ok((
        openai_image_reference_to_anthropic_source(image_reference)?,
        detail,
    ))
}

fn openai_image_reference_to_anthropic_source(
    image_reference: &str,
) -> Result<Value, GatewayError> {
    if let Some((media_type, data)) = parse_base64_data_url(image_reference) {
        return Ok(json!({
            "type": "base64",
            "media_type": media_type,
            "data": data,
        }));
    }

    Ok(json!({
        "type": "url",
        "url": image_reference,
    }))
}

fn parse_base64_data_url(value: &str) -> Option<(&str, &str)> {
    let payload = value.strip_prefix("data:")?;
    let (metadata, data) = payload.split_once(",")?;
    let media_type = metadata.strip_suffix(";base64")?;
    if media_type.is_empty() || data.is_empty() {
        return None;
    }

    Some((media_type, data))
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
