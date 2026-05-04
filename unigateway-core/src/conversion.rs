use std::collections::BTreeMap;

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

pub fn openai_messages_to_anthropic_messages(
    raw_messages: &Value,
    fallback_system: Option<Value>,
) -> Result<(Option<Value>, Value), GatewayError> {
    let Some(messages) = raw_messages.as_array() else {
        return Err(GatewayError::InvalidRequest(
            "openai messages must be an array".to_string(),
        ));
    };

    let mut system_parts = Vec::new();
    let mut anthropic_messages = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| GatewayError::InvalidRequest("message role is required".to_string()))?;

        match role {
            "system" => {
                system_parts.extend(openai_system_content_parts(message.get("content"))?);
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

    let system = if system_parts.is_empty() {
        fallback_system
    } else {
        Some(Value::String(system_parts.join("\n")))
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingOpenAiToolCall {
    pub id: String,
    pub name: String,
    pub raw_arguments: String,
    pub arguments: String,
    pub emitted_argument_len: usize,
    pub anthropic_index: Option<usize>,
    pub started: bool,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicToolUseStart {
    pub anthropic_index: usize,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicInputJsonDelta {
    pub anthropic_index: usize,
    pub partial_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiToolCallDeltaUpdate {
    pub start: Option<AnthropicToolUseStart>,
    pub delta: Option<AnthropicInputJsonDelta>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenAiToolCallStopUpdate {
    pub start: Option<AnthropicToolUseStart>,
    pub delta: Option<AnthropicInputJsonDelta>,
    pub stop_index: Option<usize>,
}

pub fn apply_openai_tool_call_delta_update(
    pending_tool_calls: &mut BTreeMap<usize, PendingOpenAiToolCall>,
    next_content_index: &mut usize,
    tool_index: usize,
    tool_call: &Value,
) -> OpenAiToolCallDeltaUpdate {
    pending_tool_calls.entry(tool_index).or_default();

    if pending_tool_calls
        .get(&tool_index)
        .and_then(|pending| pending.anthropic_index)
        .is_none()
    {
        let anthropic_index = *next_content_index;
        *next_content_index += 1;
        if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
            pending.anthropic_index = Some(anthropic_index);
        }
    }

    if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
            pending.id = id.to_string();
        }
        if let Some(name) = tool_call
            .get("function")
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
        {
            pending.name = name.to_string();
        }
        if let Some(arguments) = tool_call
            .get("function")
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str)
        {
            pending.raw_arguments =
                merge_openai_tool_call_arguments(&pending.raw_arguments, arguments);
            pending.arguments =
                normalize_openai_tool_call_arguments(&pending.raw_arguments, &pending.arguments);
        }
    }

    let mut update = OpenAiToolCallDeltaUpdate::default();
    if let Some(pending) = pending_tool_calls.get_mut(&tool_index) {
        let anthropic_index = pending.anthropic_index.unwrap_or(tool_index);
        let can_start = !pending.started && !pending.id.is_empty() && !pending.name.is_empty();
        if can_start {
            pending.started = true;
            update.start = Some(AnthropicToolUseStart {
                anthropic_index,
                id: pending.id.clone(),
                name: pending.name.clone(),
            });
        }

        if pending.started && pending.emitted_argument_len < pending.arguments.len() {
            let fragment = pending.arguments[pending.emitted_argument_len..].to_string();
            pending.emitted_argument_len = pending.arguments.len();
            update.delta = Some(AnthropicInputJsonDelta {
                anthropic_index,
                partial_json: fragment,
            });
        }
    }

    update
}

pub fn flush_openai_tool_call_stop_update(
    pending_tool_calls: &mut BTreeMap<usize, PendingOpenAiToolCall>,
    tool_index: usize,
) -> OpenAiToolCallStopUpdate {
    let Some(pending) = pending_tool_calls.get_mut(&tool_index) else {
        return OpenAiToolCallStopUpdate::default();
    };

    if pending.stopped {
        return OpenAiToolCallStopUpdate::default();
    }

    let anthropic_index = pending.anthropic_index.unwrap_or(tool_index);

    if !pending.started {
        pending.started = true;

        if pending.id.is_empty() && pending.name.is_empty() && pending.arguments.is_empty() {
            return OpenAiToolCallStopUpdate::default();
        }

        let delta = if pending.emitted_argument_len < pending.arguments.len() {
            let partial_json = pending.arguments[pending.emitted_argument_len..].to_string();
            pending.emitted_argument_len = pending.arguments.len();
            Some(AnthropicInputJsonDelta {
                anthropic_index,
                partial_json,
            })
        } else {
            None
        };

        pending.stopped = true;
        return OpenAiToolCallStopUpdate {
            start: Some(AnthropicToolUseStart {
                anthropic_index,
                id: if pending.id.is_empty() {
                    "toolu_unknown".to_string()
                } else {
                    pending.id.clone()
                },
                name: if pending.name.is_empty() {
                    "tool".to_string()
                } else {
                    pending.name.clone()
                },
            }),
            delta,
            stop_index: Some(anthropic_index),
        };
    }

    pending.stopped = true;
    OpenAiToolCallStopUpdate {
        start: None,
        delta: None,
        stop_index: Some(anthropic_index),
    }
}

fn merge_openai_tool_call_arguments(existing: &str, incoming: &str) -> String {
    if incoming.is_empty() {
        return existing.to_string();
    }
    if existing.is_empty() {
        return incoming.to_string();
    }
    if incoming.starts_with(existing) {
        return incoming.to_string();
    }
    if existing.starts_with(incoming) || existing.ends_with(incoming) {
        return existing.to_string();
    }

    let max_overlap = existing.len().min(incoming.len());
    for overlap in (1..=max_overlap).rev() {
        if existing[existing.len() - overlap..] == incoming[..overlap] {
            return format!("{existing}{}", &incoming[overlap..]);
        }
    }

    format!("{existing}{incoming}")
}

fn normalize_openai_tool_call_arguments(raw_arguments: &str, previous_arguments: &str) -> String {
    if raw_arguments.is_empty() {
        return previous_arguments.to_string();
    }

    let mut normalized = raw_arguments.to_string();
    loop {
        let repaired = strip_empty_object_prefix(&normalized);
        if repaired != normalized {
            normalized = repaired;
            continue;
        }

        if normalized.starts_with('"') {
            match serde_json::from_str::<String>(&normalized) {
                Ok(decoded) => {
                    normalized = decoded;
                    continue;
                }
                Err(_) => return previous_arguments.to_string(),
            }
        }

        return normalized;
    }
}

fn strip_empty_object_prefix(value: &str) -> String {
    let mut stripped = value;
    while stripped.starts_with("{}") && stripped.len() > 2 {
        stripped = &stripped[2..];
    }
    stripped.to_string()
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

pub fn is_placeholder_thinking_signature(signature: &str) -> bool {
    signature == THINKING_SIGNATURE_PLACEHOLDER_VALUE
}

pub fn openai_tools_to_anthropic_tools(
    tools: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
    let Some(Value::Array(items)) = tools else {
        return Ok(None);
    };

    let mut anthropic_tools = Vec::new();
    for tool in items {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }

        let function = tool.get("function").ok_or_else(|| {
            GatewayError::InvalidRequest("openai function tool requires function".to_string())
        })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GatewayError::InvalidRequest("openai function tool requires name".to_string())
            })?;

        let mut anthropic_tool = serde_json::Map::from_iter([
            ("name".to_string(), Value::String(name.to_string())),
            (
                "input_schema".to_string(),
                function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            ),
        ]);
        if let Some(description) = function.get("description").and_then(Value::as_str) {
            anthropic_tool.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        anthropic_tools.push(Value::Object(anthropic_tool));
    }

    Ok(Some(Value::Array(anthropic_tools)))
}

pub fn anthropic_tools_to_openai_tools(tools: Option<Value>) -> Option<Value> {
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

pub fn openai_tool_choice_to_anthropic_tool_choice(
    tool_choice: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
    match tool_choice {
        Some(Value::String(mode)) => match mode.as_str() {
            "auto" | "none" => Ok(Some(json!({ "type": mode }))),
            "required" => Ok(Some(json!({ "type": "any" }))),
            other => Err(GatewayError::InvalidRequest(format!(
                "unsupported openai tool_choice mode for anthropic request: {other}",
            ))),
        },
        Some(Value::Object(obj)) => match obj.get("type").and_then(Value::as_str) {
            Some("function") => obj
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .map(|name| json!({ "type": "tool", "name": name }))
                .map(Some)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "openai tool_choice function requires function.name".to_string(),
                    )
                }),
            Some("auto" | "none" | "any" | "tool") => Ok(Some(Value::Object(obj))),
            Some(other) => Err(GatewayError::InvalidRequest(format!(
                "unsupported openai tool_choice type for anthropic request: {other}",
            ))),
            None => Err(GatewayError::InvalidRequest(
                "openai tool_choice object is missing type".to_string(),
            )),
        },
        Some(other) => Err(GatewayError::InvalidRequest(format!(
            "openai tool_choice must be a string or object, got: {other}",
        ))),
        None => Ok(None),
    }
}

pub fn anthropic_tool_choice_to_openai_tool_choice(
    tool_choice: Option<Value>,
) -> Result<Option<Value>, GatewayError> {
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

fn openai_chat_message_to_anthropic_message(
    message: &Value,
    role: &str,
) -> Result<Value, GatewayError> {
    let blocks = openai_message_to_content_blocks(message)?;
    let content = content_blocks_to_anthropic_request(&blocks)?;

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

fn openai_system_content_parts(content: Option<&Value>) -> Result<Vec<String>, GatewayError> {
    match content {
        Some(Value::String(text)) => Ok(vec![text.clone()]),
        Some(Value::Array(blocks)) => {
            let parts = blocks
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
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

fn anthropic_blocks(content: Value) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({ "type": "text", "text": text })],
        Value::Array(blocks) => blocks,
        _ => Vec::new(),
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

fn json_content_to_string(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{
        AnthropicInputJsonDelta, AnthropicToolUseStart, PendingOpenAiToolCall,
        anthropic_content_to_blocks, anthropic_messages_to_openai_messages,
        anthropic_tool_choice_to_openai_tool_choice, anthropic_tools_to_openai_tools,
        apply_openai_tool_call_delta_update, content_blocks_to_anthropic,
        content_blocks_to_anthropic_request, flush_openai_tool_call_stop_update,
        openai_message_to_anthropic_content_blocks, openai_message_to_content_blocks,
        openai_messages_to_anthropic_messages, openai_tool_choice_to_anthropic_tool_choice,
        openai_tools_to_anthropic_tools, validate_anthropic_request_messages,
    };
    use crate::request::{ContentBlock, THINKING_SIGNATURE_PLACEHOLDER_VALUE};

    #[test]
    fn content_block_preserves_thinking_signature() {
        let block = ContentBlock::Thinking {
            thinking: "reasoning".to_string(),
            signature: Some("real-signature".to_string()),
        };

        let value = serde_json::to_value(&block).expect("serialize block");
        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some("thinking")
        );
        assert_eq!(
            value.get("signature").and_then(serde_json::Value::as_str),
            Some("real-signature")
        );

        let parsed: ContentBlock = serde_json::from_value(value).expect("deserialize block");
        assert_eq!(parsed, block);
    }

    #[test]
    fn content_block_preserves_tool_use_input() {
        let block = ContentBlock::ToolUse {
            id: "toolu_1".to_string(),
            name: "search".to_string(),
            input: json!({"query": "hello"}),
        };

        let value = serde_json::to_value(&block).expect("serialize block");
        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some("tool_use")
        );
        assert_eq!(value.get("input"), Some(&json!({"query": "hello"})));
    }

    #[test]
    fn openai_assistant_tool_calls_parse_to_content_blocks() {
        let blocks = openai_message_to_content_blocks(&json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "search",
                    "arguments": "{\"query\":\"rust\"}"
                }
            }]
        }))
        .expect("content blocks");

        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: json!({"query": "rust"}),
            }
        );
    }

    #[test]
    fn openai_assistant_tool_calls_ignore_empty_string_content() {
        let (system, messages) = openai_messages_to_anthropic_messages(
            &json!([
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\":\"rust\"}"
                        }
                    }]
                }
            ]),
            None,
        )
        .expect("anthropic messages");

        assert_eq!(system, None);
        assert_eq!(
            messages
                .pointer("/0/content/0/type")
                .and_then(Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            messages.pointer("/0/content/1").and_then(Value::as_object),
            None
        );
    }

    #[test]
    fn openai_tool_message_parses_to_tool_result_block() {
        let blocks = openai_message_to_content_blocks(&json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "search result"
        }))
        .expect("content blocks");

        assert_eq!(
            blocks,
            vec![ContentBlock::ToolResult {
                tool_use_id: "call_1".to_string(),
                content: "search result".to_string(),
            }]
        );
    }

    #[test]
    fn anthropic_tool_use_serializes_to_openai_tool_call() {
        let block = ContentBlock::ToolUse {
            id: "toolu_1".to_string(),
            name: "search".to_string(),
            input: json!({"query": "rust"}),
        };

        let tool_call = block
            .to_openai_tool_call()
            .expect("tool call")
            .expect("some tool call");

        assert_eq!(
            tool_call.get("id").and_then(serde_json::Value::as_str),
            Some("toolu_1")
        );
        assert_eq!(
            tool_call
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(serde_json::Value::as_str),
            Some("{\"query\":\"rust\"}")
        );
    }

    #[test]
    fn anthropic_tool_result_text_blocks_parse_to_openai_content() {
        let blocks = anthropic_content_to_blocks(&json!([{
            "type": "tool_result",
            "tool_use_id": "toolu_1",
            "content": [{"type": "text", "text": "first"}, {"text": "second"}]
        }]))
        .expect("content blocks");

        assert_eq!(
            blocks,
            vec![ContentBlock::ToolResult {
                tool_use_id: "toolu_1".to_string(),
                content: "first\nsecond".to_string(),
            }]
        );
        assert_eq!(
            blocks[0]
                .to_openai_tool_message()
                .and_then(|message| message.get("content").cloned())
                .and_then(|content| content.as_str().map(str::to_string)),
            Some("first\nsecond".to_string())
        );
    }

    #[test]
    fn openai_function_tools_convert_to_anthropic_tools() {
        let tools = openai_tools_to_anthropic_tools(Some(json!([{
            "type": "function",
            "function": {
                "name": "lookup_weather",
                "description": "Look up weather",
                "parameters": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }
        }])))
        .expect("converted tools")
        .expect("tools");

        assert_eq!(
            tools
                .as_array()
                .and_then(|items| items.first())
                .and_then(|tool| tool.get("name"))
                .and_then(serde_json::Value::as_str),
            Some("lookup_weather")
        );
        assert_eq!(
            tools
                .as_array()
                .and_then(|items| items.first())
                .and_then(|tool| tool.get("input_schema"))
                .and_then(|schema| schema.get("required"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn anthropic_tools_convert_to_openai_function_tools() {
        let tools = anthropic_tools_to_openai_tools(Some(json!([{
            "name": "lookup_weather",
            "description": "Look up weather",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}}
            }
        }])))
        .expect("tools");

        assert_eq!(
            tools
                .as_array()
                .and_then(|items| items.first())
                .and_then(|tool| tool.get("type"))
                .and_then(serde_json::Value::as_str),
            Some("function")
        );
        assert_eq!(
            tools
                .as_array()
                .and_then(|items| items.first())
                .and_then(|tool| tool.get("function"))
                .and_then(|function| function.get("name"))
                .and_then(serde_json::Value::as_str),
            Some("lookup_weather")
        );
    }

    #[test]
    fn tool_choice_converts_between_openai_and_anthropic_shapes() {
        let anthropic_choice = openai_tool_choice_to_anthropic_tool_choice(Some(json!({
            "type": "function",
            "function": {"name": "lookup_weather"}
        })))
        .expect("converted to anthropic")
        .expect("choice");
        assert_eq!(
            anthropic_choice
                .get("type")
                .and_then(serde_json::Value::as_str),
            Some("tool")
        );
        assert_eq!(
            anthropic_choice
                .get("name")
                .and_then(serde_json::Value::as_str),
            Some("lookup_weather")
        );

        let openai_choice = anthropic_tool_choice_to_openai_tool_choice(Some(anthropic_choice))
            .expect("converted to openai")
            .expect("choice");
        assert_eq!(
            openai_choice
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(serde_json::Value::as_str),
            Some("lookup_weather")
        );
    }

    #[test]
    fn anthropic_content_blocks_preserve_thinking_signature() {
        let blocks = anthropic_content_to_blocks(&json!([
            {
                "type": "thinking",
                "thinking": "plan",
                "signature": "real-signature"
            },
            {"type": "text", "text": "answer"}
        ]))
        .expect("content blocks");

        assert_eq!(
            blocks,
            vec![
                ContentBlock::Thinking {
                    thinking: "plan".to_string(),
                    signature: Some("real-signature".to_string()),
                },
                ContentBlock::Text {
                    text: "answer".to_string(),
                },
            ]
        );
    }

    #[test]
    fn content_blocks_serialize_to_anthropic_blocks() {
        let blocks = vec![
            ContentBlock::Thinking {
                thinking: "plan".to_string(),
                signature: Some("real-signature".to_string()),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: json!({"query": "rust"}),
            },
        ];

        let values = content_blocks_to_anthropic(&blocks);
        assert_eq!(
            values[0].get("type").and_then(serde_json::Value::as_str),
            Some("thinking")
        );
        assert_eq!(
            values[0]
                .get("signature")
                .and_then(serde_json::Value::as_str),
            Some("real-signature")
        );
        assert_eq!(
            values[1].get("type").and_then(serde_json::Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            values[1]
                .get("input")
                .and_then(|input| input.get("query"))
                .and_then(serde_json::Value::as_str),
            Some("rust")
        );
    }

    #[test]
    fn content_blocks_reject_placeholder_signature_for_anthropic_request() {
        let blocks = vec![ContentBlock::Thinking {
            thinking: "plan".to_string(),
            signature: Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE.to_string()),
        }];

        let error = content_blocks_to_anthropic_request(&blocks).expect_err("placeholder rejected");
        assert!(matches!(
            error,
            crate::error::GatewayError::InvalidRequest(_)
        ));
    }

    #[test]
    fn anthropic_request_messages_reject_placeholder_signature() {
        let error = validate_anthropic_request_messages(&json!([{
            "role": "assistant",
            "content": [{
                "type": "thinking",
                "thinking": "plan",
                "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE
            }]
        }]))
        .expect_err("placeholder rejected");

        assert!(matches!(
            error,
            crate::error::GatewayError::InvalidRequest(_)
        ));
    }

    #[test]
    fn openai_messages_convert_to_anthropic_messages() {
        let (system, messages) = openai_messages_to_anthropic_messages(
            &json!([
                {"role": "system", "content": "be precise"},
                {"role": "user", "content": "find rust examples"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{\"query\":\"rust examples\"}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "result text"
                }
            ]),
            None,
        )
        .expect("anthropic messages");

        assert_eq!(system, Some(Value::String("be precise".to_string())));
        assert_eq!(
            messages
                .pointer("/0/content/0/text")
                .and_then(Value::as_str),
            Some("find rust examples")
        );
        assert_eq!(
            messages
                .pointer("/1/content/0/type")
                .and_then(Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            messages
                .pointer("/2/content/0/type")
                .and_then(Value::as_str),
            Some("tool_result")
        );
    }

    #[test]
    fn anthropic_messages_convert_to_openai_messages() {
        let messages = anthropic_messages_to_openai_messages(&json!([
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "plan"},
                    {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"query": "rust"}},
                    {"type": "text", "text": "done"}
                ]
            },
            {
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "result text"}
                ]
            }
        ]))
        .expect("openai messages");

        assert_eq!(
            messages[0].get("thinking").and_then(Value::as_str),
            Some("plan")
        );
        assert_eq!(
            messages[0]
                .get("tool_calls")
                .and_then(|tool_calls| tool_calls.as_array())
                .and_then(|items| items.first())
                .and_then(|tool| tool.get("id"))
                .and_then(Value::as_str),
            Some("toolu_1")
        );
        assert_eq!(
            messages[1].get("role").and_then(Value::as_str),
            Some("tool")
        );
    }

    #[test]
    fn openai_message_downstream_blocks_include_thinking_and_tool_use() {
        let blocks = openai_message_to_anthropic_content_blocks(&json!({
            "role": "assistant",
            "reasoning_content": "need weather first",
            "content": "I'll call a tool",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "lookup_weather",
                    "arguments": "{\"city\":\"Paris\"}"
                }
            }]
        }));

        assert_eq!(
            blocks[0].get("type").and_then(Value::as_str),
            Some("thinking")
        );
        assert_eq!(
            blocks[0].get("signature").and_then(Value::as_str),
            Some(THINKING_SIGNATURE_PLACEHOLDER_VALUE)
        );
        assert_eq!(
            blocks[1].get("text").and_then(Value::as_str),
            Some("I'll call a tool")
        );
        assert_eq!(
            blocks[2].get("type").and_then(Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            blocks[2]
                .get("input")
                .and_then(|input| input.get("city"))
                .and_then(Value::as_str),
            Some("Paris")
        );
    }

    #[test]
    fn openai_message_downstream_blocks_ignore_non_text_array_items() {
        let blocks = openai_message_to_anthropic_content_blocks(&json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
            ]
        }));

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].get("text").and_then(Value::as_str), Some("hello"));
    }

    #[test]
    fn tool_call_delta_update_emits_start_and_delta() {
        let mut pending_tool_calls = BTreeMap::new();
        let mut next_content_index = 2;

        let update = apply_openai_tool_call_delta_update(
            &mut pending_tool_calls,
            &mut next_content_index,
            0,
            &json!({
                "id": "call_1",
                "function": {
                    "name": "lookup_weather",
                    "arguments": "{\"city\":\"Paris\"}"
                }
            }),
        );

        assert_eq!(next_content_index, 3);
        assert_eq!(
            update.start,
            Some(AnthropicToolUseStart {
                anthropic_index: 2,
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
            })
        );
        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "{\"city\":\"Paris\"}".to_string(),
            })
        );
        assert_eq!(
            pending_tool_calls.get(&0),
            Some(&PendingOpenAiToolCall {
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                raw_arguments: "{\"city\":\"Paris\"}".to_string(),
                arguments: "{\"city\":\"Paris\"}".to_string(),
                emitted_argument_len: 16,
                anthropic_index: Some(2),
                started: true,
                stopped: false,
            })
        );
    }

    #[test]
    fn tool_call_delta_update_only_emits_new_argument_fragment() {
        let mut pending_tool_calls = BTreeMap::from([(
            0,
            PendingOpenAiToolCall {
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                raw_arguments: "{\"city\":\"".to_string(),
                arguments: "{\"city\":\"".to_string(),
                emitted_argument_len: 9,
                anthropic_index: Some(2),
                started: true,
                stopped: false,
            },
        )]);
        let mut next_content_index = 3;

        let update = apply_openai_tool_call_delta_update(
            &mut pending_tool_calls,
            &mut next_content_index,
            0,
            &json!({
                "function": {
                    "arguments": "Paris\"}"
                }
            }),
        );

        assert_eq!(next_content_index, 3);
        assert_eq!(update.start, None);
        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "Paris\"}".to_string(),
            })
        );
    }

    #[test]
    fn tool_call_stop_update_emits_placeholder_start_and_buffered_delta() {
        let mut pending_tool_calls = BTreeMap::from([(
            0,
            PendingOpenAiToolCall {
                id: String::new(),
                name: String::new(),
                raw_arguments: "{\"city\":\"Paris\"}".to_string(),
                arguments: "{\"city\":\"Paris\"}".to_string(),
                emitted_argument_len: 0,
                anthropic_index: Some(2),
                started: false,
                stopped: false,
            },
        )]);

        let update = flush_openai_tool_call_stop_update(&mut pending_tool_calls, 0);

        assert_eq!(
            update.start,
            Some(AnthropicToolUseStart {
                anthropic_index: 2,
                id: "toolu_unknown".to_string(),
                name: "tool".to_string(),
            })
        );
        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "{\"city\":\"Paris\"}".to_string(),
            })
        );
        assert_eq!(update.stop_index, Some(2));
        assert_eq!(
            pending_tool_calls.get(&0),
            Some(&PendingOpenAiToolCall {
                id: String::new(),
                name: String::new(),
                raw_arguments: "{\"city\":\"Paris\"}".to_string(),
                arguments: "{\"city\":\"Paris\"}".to_string(),
                emitted_argument_len: 16,
                anthropic_index: Some(2),
                started: true,
                stopped: true,
            })
        );
    }

    #[test]
    fn tool_call_stop_update_emits_stop_only_for_started_call() {
        let mut pending_tool_calls = BTreeMap::from([(
            0,
            PendingOpenAiToolCall {
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                raw_arguments: "{\"city\":\"Paris\"}".to_string(),
                arguments: "{\"city\":\"Paris\"}".to_string(),
                emitted_argument_len: 16,
                anthropic_index: Some(2),
                started: true,
                stopped: false,
            },
        )]);

        let update = flush_openai_tool_call_stop_update(&mut pending_tool_calls, 0);

        assert_eq!(update.start, None);
        assert_eq!(update.delta, None);
        assert_eq!(update.stop_index, Some(2));
        assert_eq!(
            pending_tool_calls.get(&0).map(|pending| pending.stopped),
            Some(true)
        );
    }

    #[test]
    fn tool_call_delta_update_deduplicates_cumulative_argument_snapshots() {
        let mut pending_tool_calls = BTreeMap::from([(
            0,
            PendingOpenAiToolCall {
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                raw_arguments: "{\"city\":\"".to_string(),
                arguments: "{\"city\":\"".to_string(),
                emitted_argument_len: 9,
                anthropic_index: Some(2),
                started: true,
                stopped: false,
            },
        )]);
        let mut next_content_index = 3;

        let update = apply_openai_tool_call_delta_update(
            &mut pending_tool_calls,
            &mut next_content_index,
            0,
            &json!({
                "function": {
                    "arguments": "{\"city\":\"Paris\"}"
                }
            }),
        );

        assert_eq!(update.start, None);
        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "Paris\"}".to_string(),
            })
        );
        assert_eq!(
            pending_tool_calls
                .get(&0)
                .map(|pending| pending.arguments.as_str()),
            Some("{\"city\":\"Paris\"}")
        );
    }

    #[test]
    fn tool_call_delta_update_normalizes_double_encoded_json_string() {
        let mut pending_tool_calls = BTreeMap::new();
        let mut next_content_index = 2;

        let update = apply_openai_tool_call_delta_update(
            &mut pending_tool_calls,
            &mut next_content_index,
            0,
            &json!({
                "id": "call_1",
                "function": {
                    "name": "lookup_weather",
                    "arguments": "\"{\\\"city\\\":\\\"Paris\\\"}\""
                }
            }),
        );

        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "{\"city\":\"Paris\"}".to_string(),
            })
        );
        assert_eq!(
            pending_tool_calls
                .get(&0)
                .map(|pending| pending.arguments.as_str()),
            Some("{\"city\":\"Paris\"}")
        );
    }

    #[test]
    fn tool_call_delta_update_strips_empty_object_prefix() {
        let mut pending_tool_calls = BTreeMap::new();
        let mut next_content_index = 2;

        let update = apply_openai_tool_call_delta_update(
            &mut pending_tool_calls,
            &mut next_content_index,
            0,
            &json!({
                "id": "call_1",
                "function": {
                    "name": "lookup_weather",
                    "arguments": "{}{\"city\":\"Paris\"}"
                }
            }),
        );

        assert_eq!(
            update.delta,
            Some(AnthropicInputJsonDelta {
                anthropic_index: 2,
                partial_json: "{\"city\":\"Paris\"}".to_string(),
            })
        );
        assert_eq!(
            pending_tool_calls
                .get(&0)
                .map(|pending| pending.arguments.as_str()),
            Some("{\"city\":\"Paris\"}")
        );
    }
}
