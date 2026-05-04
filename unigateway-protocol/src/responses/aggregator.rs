use anyhow::{Result, anyhow};
use serde_json::Value;
use std::collections::BTreeMap;

use unigateway_core::ChatResponseChunk;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnthropicStreamAggregator {
    message_id: Option<String>,
    model: Option<String>,
    role: Option<String>,
    stop_reason: Option<String>,
    stop_sequence: Option<Value>,
    usage: Option<Value>,
    completed: bool,
    content_blocks: BTreeMap<usize, AggregatedAnthropicContentBlock>,
}

#[derive(Debug, Clone, PartialEq)]
enum AggregatedAnthropicContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
}

impl AnthropicStreamAggregator {
    pub fn push_event(&mut self, event_type: &str, data: &Value) -> Result<()> {
        let event_type = if event_type.is_empty() {
            data.get("type").and_then(Value::as_str).unwrap_or_default()
        } else {
            event_type
        };

        match event_type {
            "message_start" => {
                let message = data.get("message").unwrap_or(data);
                self.message_id = message
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.model = message
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "content_block_start" => {
                let index = content_block_index(data)?;
                let content_block = data.get("content_block").ok_or_else(|| {
                    anyhow!("anthropic content_block_start is missing content_block")
                })?;
                let block_type = content_block
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anyhow!("anthropic content_block_start is missing content_block.type")
                    })?;

                let block = match block_type {
                    "text" => AggregatedAnthropicContentBlock::Text {
                        text: content_block
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    },
                    "thinking" => AggregatedAnthropicContentBlock::Thinking {
                        thinking: content_block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        signature: content_block
                            .get("signature")
                            .and_then(Value::as_str)
                            .filter(|signature| !signature.is_empty())
                            .map(str::to_string),
                    },
                    "tool_use" => AggregatedAnthropicContentBlock::ToolUse {
                        id: content_block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: content_block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        input_json: match content_block.get("input") {
                            Some(Value::Object(map)) if map.is_empty() => String::new(),
                            Some(Value::Object(_)) | Some(Value::Array(_)) => {
                                serde_json::to_string(
                                    content_block.get("input").unwrap_or(&Value::Null),
                                )
                                .unwrap_or_else(|_| "{}".to_string())
                            }
                            Some(Value::Null) | None => String::new(),
                            Some(other) => {
                                serde_json::to_string(other).unwrap_or_else(|_| String::new())
                            }
                        },
                    },
                    _ => return Ok(()),
                };

                self.content_blocks.insert(index, block);
            }
            "content_block_delta" => {
                let index = content_block_index(data)?;
                let delta = data
                    .get("delta")
                    .ok_or_else(|| anyhow!("anthropic content_block_delta is missing delta"))?;
                match delta
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "text_delta" => {
                        let text = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match self.content_blocks.get_mut(&index) {
                            Some(AggregatedAnthropicContentBlock::Text { text: existing }) => {
                                existing.push_str(text);
                            }
                            _ => {
                                self.content_blocks.insert(
                                    index,
                                    AggregatedAnthropicContentBlock::Text {
                                        text: text.to_string(),
                                    },
                                );
                            }
                        }
                    }
                    "thinking_delta" => {
                        let thinking = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match self.content_blocks.get_mut(&index) {
                            Some(AggregatedAnthropicContentBlock::Thinking {
                                thinking: existing,
                                ..
                            }) => existing.push_str(thinking),
                            _ => {
                                self.content_blocks.insert(
                                    index,
                                    AggregatedAnthropicContentBlock::Thinking {
                                        thinking: thinking.to_string(),
                                        signature: None,
                                    },
                                );
                            }
                        }
                    }
                    "signature_delta" => {
                        let signature = delta
                            .get("signature")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match self.content_blocks.get_mut(&index) {
                            Some(AggregatedAnthropicContentBlock::Thinking {
                                signature: existing,
                                ..
                            }) => {
                                *existing = Some(signature.to_string());
                            }
                            _ => {
                                self.content_blocks.insert(
                                    index,
                                    AggregatedAnthropicContentBlock::Thinking {
                                        thinking: String::new(),
                                        signature: Some(signature.to_string()),
                                    },
                                );
                            }
                        }
                    }
                    "input_json_delta" => {
                        let partial_json = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match self.content_blocks.get_mut(&index) {
                            Some(AggregatedAnthropicContentBlock::ToolUse {
                                input_json, ..
                            }) => {
                                input_json.push_str(partial_json);
                            }
                            _ => {
                                self.content_blocks.insert(
                                    index,
                                    AggregatedAnthropicContentBlock::ToolUse {
                                        id: String::new(),
                                        name: String::new(),
                                        input_json: partial_json.to_string(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                self.stop_reason = data
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.stop_reason.clone());
                self.stop_sequence = data
                    .get("delta")
                    .and_then(|delta| delta.get("stop_sequence"))
                    .cloned()
                    .or_else(|| self.stop_sequence.clone());
                self.usage = data.get("usage").cloned().or_else(|| self.usage.clone());
            }
            "message_stop" => {
                self.completed = true;
            }
            "content_block_stop" | "ping" => {}
            _ => {}
        }

        Ok(())
    }

    pub fn push_chunk(&mut self, chunk: &ChatResponseChunk) -> Result<()> {
        let event_type = chunk
            .raw
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.push_event(event_type, &chunk.raw)
    }

    pub fn is_complete(&self) -> bool {
        self.completed
    }

    pub fn snapshot_message(&self) -> Result<Value> {
        let content = self
            .content_blocks
            .values()
            .map(AggregatedAnthropicContentBlock::to_value)
            .collect::<Result<Vec<_>>>()?;

        Ok(serde_json::json!({
            "id": self.message_id,
            "type": "message",
            "role": self.role.clone().unwrap_or_else(|| "assistant".to_string()),
            "model": self.model,
            "content": content,
            "stop_reason": self.stop_reason,
            "stop_sequence": self.stop_sequence.clone().unwrap_or(Value::Null),
            "usage": self.usage.clone().unwrap_or_else(|| serde_json::json!({})),
        }))
    }

    pub fn into_message(self) -> Result<Value> {
        self.snapshot_message()
    }
}

impl AggregatedAnthropicContentBlock {
    fn to_value(&self) -> Result<Value> {
        match self {
            Self::Text { text } => Ok(serde_json::json!({
                "type": "text",
                "text": text,
            })),
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
                Ok(Value::Object(block))
            }
            Self::ToolUse {
                id,
                name,
                input_json,
            } => {
                let input = if input_json.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_str(input_json).map_err(|error| {
                        anyhow!("failed to parse aggregated anthropic tool_use input JSON: {error}")
                    })?
                };
                Ok(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                }))
            }
        }
    }
}

fn content_block_index(data: &Value) -> Result<usize> {
    data.get("index")
        .and_then(Value::as_u64)
        .map(|index| index as usize)
        .ok_or_else(|| anyhow!("anthropic content block event is missing index"))
}
