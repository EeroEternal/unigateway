use std::collections::BTreeMap;

use bytes::Bytes;
use serde_json::Value;

use unigateway_core::ChatResponseChunk;

use super::reasoning_text::{ReasoningTextChunk, ReasoningTextEncoding, ReasoningTextStreamParser};

#[derive(Default)]
pub struct OpenAiChatStreamAdapter {
    pub(crate) model: Option<String>,
    pub(crate) sent_role_chunk: bool,
    reasoning_text_encoding: Option<ReasoningTextEncoding>,
    reasoning_text_parsers: BTreeMap<usize, ReasoningTextStreamParser>,
}

impl OpenAiChatStreamAdapter {
    pub(crate) fn with_reasoning_text_encoding(
        reasoning_text_encoding: Option<ReasoningTextEncoding>,
    ) -> Self {
        Self {
            reasoning_text_encoding,
            ..Self::default()
        }
    }

    pub(crate) fn finish_reasoning_text(&mut self, request_id: &str) -> Vec<Bytes> {
        let mut chunks = Vec::new();
        for (choice_index, parser) in &mut self.reasoning_text_parsers {
            chunks.extend(reasoning_text_chunks_to_openai_bytes(
                request_id,
                self.model.as_deref().unwrap_or_default(),
                *choice_index,
                parser.finish(),
            ));
        }
        chunks
    }

    fn reasoning_text_parser_for_choice(
        &mut self,
        choice_index: usize,
    ) -> Option<&mut ReasoningTextStreamParser> {
        let encoding = self.reasoning_text_encoding?;
        Some(
            self.reasoning_text_parsers
                .entry(choice_index)
                .or_insert_with(|| ReasoningTextStreamParser::new(encoding)),
        )
    }
}

pub fn openai_sse_chunks_from_chat_chunk(
    request_id: &str,
    adapter: &mut OpenAiChatStreamAdapter,
    chunk: ChatResponseChunk,
) -> Vec<Bytes> {
    if chunk.raw.get("choices").and_then(Value::as_array).is_some() {
        return openai_compatible_sse_chunks_from_raw(adapter, &chunk.raw);
    }

    let event_type = chunk
        .raw
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match event_type {
        "message_start" => {
            adapter.model = chunk
                .raw
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    chunk
                        .raw
                        .get("message")
                        .and_then(|message| message.get("model"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });

            if adapter.sent_role_chunk {
                return Vec::new();
            }

            adapter.sent_role_chunk = true;
            vec![openai_chat_sse_bytes(
                request_id,
                adapter.model.as_deref().unwrap_or_default(),
                serde_json::json!({"role": "assistant"}),
                None,
            )]
        }
        "content_block_delta" => {
            let delta = chunk.raw.get("delta");
            if let Some(text) = delta.and_then(|d| d.get("text")).and_then(Value::as_str) {
                return emit_openai_text_delta(request_id, adapter, 0, text);
            }

            if let Some(thinking) = delta.and_then(|d| d.get("thinking")).and_then(Value::as_str) {
                return emit_openai_thinking_delta(request_id, adapter, 0, thinking);
            }

            Vec::new()
        }
        "message_stop" => {
            let mut chunks = adapter.finish_reasoning_text(request_id);
            chunks.push(openai_chat_sse_bytes(
                request_id,
                adapter.model.as_deref().unwrap_or_default(),
                serde_json::json!({}),
                Some("stop"),
            ));
            chunks
        }
        _ => Vec::new(),
    }
}

fn openai_compatible_sse_chunks_from_raw(
    adapter: &mut OpenAiChatStreamAdapter,
    raw: &Value,
) -> Vec<Bytes> {
    if adapter.reasoning_text_encoding.is_none() {
        return sse_bytes_from_json(raw);
    }

    adapter.model = raw
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| adapter.model.clone());

    let Some(choices) = raw.get("choices").and_then(Value::as_array) else {
        return sse_bytes_from_json(raw);
    };

    let mut chunks = Vec::new();
    for (fallback_index, choice) in choices.iter().enumerate() {
        chunks.extend(openai_compatible_sse_chunks_from_choice(
            adapter,
            raw,
            choice,
            fallback_index,
        ));
    }
    chunks
}

fn openai_compatible_sse_chunks_from_choice(
    adapter: &mut OpenAiChatStreamAdapter,
    raw: &Value,
    choice: &Value,
    fallback_index: usize,
) -> Vec<Bytes> {
    let choice_index = choice
        .get("index")
        .and_then(Value::as_u64)
        .map(|index| index as usize)
        .unwrap_or(fallback_index);
    let delta = choice.get("delta");
    let content = delta
        .and_then(|delta| delta.get("content"))
        .and_then(Value::as_str);
    let finish_reason = choice.get("finish_reason").and_then(Value::as_str);

    let Some(content) = content else {
        let mut chunks = if finish_reason.is_some() {
            flush_reasoning_text_raw_choice(adapter, raw, choice, choice_index)
        } else {
            Vec::new()
        };
        chunks.extend(sse_bytes_from_raw_choice(raw, choice.clone()));
        return chunks;
    };

    let mut chunks = Vec::new();
    if let Some(non_content_delta) = delta.and_then(delta_without_content)
        && let Some(choice) = choice_with_delta(choice, non_content_delta, None)
    {
        chunks.extend(sse_bytes_from_raw_choice(raw, choice));
    }

    if let Some(parser) = adapter.reasoning_text_parser_for_choice(choice_index) {
        chunks.extend(reasoning_text_chunks_to_raw_openai_bytes(
            raw,
            choice,
            parser.push(content),
        ));
    }

    if finish_reason.is_some() {
        chunks.extend(flush_reasoning_text_raw_choice(
            adapter,
            raw,
            choice,
            choice_index,
        ));
        if let Some(choice) = choice_with_delta(choice, serde_json::json!({}), finish_reason) {
            chunks.extend(sse_bytes_from_raw_choice(raw, choice));
        }
    }

    chunks
}

fn emit_openai_text_delta(
    request_id: &str,
    adapter: &mut OpenAiChatStreamAdapter,
    choice_index: usize,
    text: &str,
) -> Vec<Bytes> {
    let mut chunks = Vec::new();
    if !adapter.sent_role_chunk {
        adapter.sent_role_chunk = true;
        chunks.push(openai_chat_sse_bytes(
            request_id,
            adapter.model.as_deref().unwrap_or_default(),
            serde_json::json!({"role": "assistant"}),
            None,
        ));
    }

    let model = adapter.model.clone().unwrap_or_default();
    let Some(parser) = adapter.reasoning_text_parser_for_choice(choice_index) else {
        chunks.push(openai_chat_sse_bytes_for_choice(
            request_id,
            &model,
            choice_index,
            serde_json::json!({"content": text}),
            None,
        ));
        return chunks;
    };

    chunks.extend(reasoning_text_chunks_to_openai_bytes(
        request_id,
        &model,
        choice_index,
        parser.push(text),
    ));
    chunks
}

fn emit_openai_thinking_delta(
    request_id: &str,
    adapter: &mut OpenAiChatStreamAdapter,
    choice_index: usize,
    thinking: &str,
) -> Vec<Bytes> {
    let mut chunks = Vec::new();
    if !adapter.sent_role_chunk {
        adapter.sent_role_chunk = true;
        chunks.push(openai_chat_sse_bytes(
            request_id,
            adapter.model.as_deref().unwrap_or_default(),
            serde_json::json!({"role": "assistant"}),
            None,
        ));
    }

    let model = adapter.model.clone().unwrap_or_default();
    chunks.push(openai_chat_sse_bytes_for_choice(
        request_id,
        &model,
        choice_index,
        serde_json::json!({
            "reasoning_content": thinking,
            "thinking": thinking,
        }),
        None,
    ));
    chunks
}

fn flush_reasoning_text_raw_choice(
    adapter: &mut OpenAiChatStreamAdapter,
    raw: &Value,
    choice: &Value,
    choice_index: usize,
) -> Vec<Bytes> {
    let Some(parser) = adapter.reasoning_text_parsers.get_mut(&choice_index) else {
        return Vec::new();
    };
    reasoning_text_chunks_to_raw_openai_bytes(raw, choice, parser.finish())
}

fn reasoning_text_chunks_to_openai_bytes(
    request_id: &str,
    model: &str,
    choice_index: usize,
    chunks: Vec<ReasoningTextChunk>,
) -> Vec<Bytes> {
    chunks
        .into_iter()
        .map(|chunk| match chunk {
            ReasoningTextChunk::Thinking(thinking) => openai_chat_sse_bytes_for_choice(
                request_id,
                model,
                choice_index,
                serde_json::json!({"reasoning_content": thinking, "thinking": thinking}),
                None,
            ),
            ReasoningTextChunk::Text(text) => openai_chat_sse_bytes_for_choice(
                request_id,
                model,
                choice_index,
                serde_json::json!({"content": text}),
                None,
            ),
        })
        .collect()
}

fn reasoning_text_chunks_to_raw_openai_bytes(
    raw: &Value,
    choice: &Value,
    chunks: Vec<ReasoningTextChunk>,
) -> Vec<Bytes> {
    chunks
        .into_iter()
        .flat_map(|chunk| {
            let delta = match chunk {
                ReasoningTextChunk::Thinking(thinking) => {
                    serde_json::json!({"reasoning_content": thinking, "thinking": thinking})
                }
                ReasoningTextChunk::Text(text) => serde_json::json!({"content": text}),
            };
            choice_with_delta(choice, delta, None)
                .map(|choice| sse_bytes_from_raw_choice(raw, choice))
                .unwrap_or_default()
        })
        .collect()
}

fn delta_without_content(delta: &Value) -> Option<Value> {
    let mut object = delta.as_object()?.clone();
    object.remove("content");
    if object.is_empty() {
        None
    } else {
        Some(Value::Object(object))
    }
}

fn choice_with_delta(choice: &Value, delta: Value, finish_reason: Option<&str>) -> Option<Value> {
    let mut choice = choice.as_object()?.clone();
    choice.insert("delta".to_string(), delta);
    choice.insert(
        "finish_reason".to_string(),
        finish_reason
            .map(|reason| Value::String(reason.to_string()))
            .unwrap_or(Value::Null),
    );
    Some(Value::Object(choice))
}

fn sse_bytes_from_raw_choice(raw: &Value, choice: Value) -> Vec<Bytes> {
    let Some(mut payload) = raw.as_object().cloned() else {
        return Vec::new();
    };
    payload.insert("choices".to_string(), Value::Array(vec![choice]));
    sse_bytes_from_json(&Value::Object(payload))
}

fn sse_bytes_from_json(value: &Value) -> Vec<Bytes> {
    serde_json::to_string(value)
        .map(|json| vec![Bytes::from(format!("data: {json}\n\n"))])
        .unwrap_or_default()
}

fn openai_chat_sse_bytes(
    request_id: &str,
    model: &str,
    delta: serde_json::Value,
    finish_reason: Option<&str>,
) -> Bytes {
    openai_chat_sse_bytes_for_choice(request_id, model, 0, delta, finish_reason)
}

fn openai_chat_sse_bytes_for_choice(
    request_id: &str,
    model: &str,
    choice_index: usize,
    delta: serde_json::Value,
    finish_reason: Option<&str>,
) -> Bytes {
    let payload = serde_json::json!({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{
            "index": choice_index,
            "delta": delta,
            "finish_reason": finish_reason,
        }],
    });

    Bytes::from(format!("data: {}\n\n", payload))
}
