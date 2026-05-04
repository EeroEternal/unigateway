use std::collections::BTreeMap;
use std::io;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProviderKind,
    ProxySession, ResponsesEvent, ResponsesFinal, StreamingResponse,
    THINKING_SIGNATURE_PLACEHOLDER_VALUE, TokenUsage,
    conversion::{
        PendingOpenAiToolCall, apply_openai_tool_call_delta_update,
        flush_openai_tool_call_stop_update, openai_message_to_anthropic_content_blocks,
    },
};

use crate::{ProtocolHttpResponse, anthropic_requested_model_alias_or};

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

#[derive(Default)]
pub struct OpenAiChatStreamAdapter {
    model: Option<String>,
    sent_role_chunk: bool,
}

pub fn render_openai_chat_session(
    session: ProxySession<ChatResponseChunk, ChatResponseFinal>,
) -> ProtocolHttpResponse {
    match session {
        ProxySession::Completed(result) => {
            ProtocolHttpResponse::ok_json(openai_completed_chat_body(result))
        }
        ProxySession::Streaming(streaming) => {
            let StreamingResponse {
                stream,
                completion,
                request_id,
                request_metadata,
            } = streaming;
            let stream_request_id = request_id.clone();
            let adapter_state =
                std::sync::Arc::new(std::sync::Mutex::new(OpenAiChatStreamAdapter::default()));

            let stream = stream.flat_map(move |item| {
                let request_id = stream_request_id.clone();
                let adapter_state = adapter_state.clone();

                let chunks: Vec<Result<Bytes, io::Error>> = match item {
                    Ok(chunk) => {
                        let mut adapter = adapter_state.lock().expect("adapter lock");
                        openai_sse_chunks_from_chat_chunk(&request_id, &mut adapter, chunk)
                            .into_iter()
                            .map(Ok)
                            .collect()
                    }
                    Err(error) => vec![Err(io::Error::other(error.to_string()))],
                };

                futures_util::stream::iter(chunks)
            });
            let done = futures_util::stream::once(async {
                Ok::<Bytes, io::Error>(Bytes::from("data: [DONE]\n\n"))
            });
            let completion_streaming = StreamingResponse {
                stream: Box::pin(futures_util::stream::empty::<
                    Result<ChatResponseChunk, unigateway_core::GatewayError>,
                >()),
                completion,
                request_id: request_id.clone(),
                request_metadata,
            };
            tokio::spawn(async move {
                let _ = completion_streaming.into_completion().await;
            });

            ProtocolHttpResponse::ok_sse(Box::pin(stream.chain(done)))
        }
    }
}

pub fn render_anthropic_chat_session(
    session: ProxySession<ChatResponseChunk, ChatResponseFinal>,
) -> ProtocolHttpResponse {
    match session {
        ProxySession::Completed(result) => {
            ProtocolHttpResponse::ok_json(anthropic_completed_chat_body(result))
        }
        ProxySession::Streaming(streaming) => {
            let requested_model = anthropic_requested_model_alias_or(
                &streaming.request_metadata,
                streaming.request_id.as_str(),
            );
            let (sender, receiver) = mpsc::channel(16);
            tokio::spawn(async move {
                drive_anthropic_chat_stream(streaming, requested_model, sender).await;
            });

            let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|item| (item, receiver))
            });

            ProtocolHttpResponse::ok_sse(Box::pin(stream))
        }
    }
}

pub fn render_openai_responses_session(
    session: ProxySession<ResponsesEvent, ResponsesFinal>,
) -> ProtocolHttpResponse {
    match session {
        ProxySession::Completed(result) => {
            let raw = result.response.raw;
            let body = if raw.is_object() {
                raw
            } else {
                serde_json::json!({
                    "id": result.report.request_id,
                    "object": "response",
                    "output_text": result.response.output_text,
                    "usage": result.report.usage.as_ref().map(|usage| serde_json::json!({
                        "input_tokens": usage.input_tokens,
                        "output_tokens": usage.output_tokens,
                        "total_tokens": usage.total_tokens,
                    })),
                })
            };
            ProtocolHttpResponse::ok_json(body)
        }
        ProxySession::Streaming(streaming) => {
            let StreamingResponse {
                stream,
                completion,
                request_id,
                request_metadata,
            } = streaming;
            let stream = stream.map(|item| match item {
                Ok(event) => {
                    let mut data = event.data;
                    if let Some(object) = data.as_object_mut() {
                        object
                            .entry("type".to_string())
                            .or_insert_with(|| serde_json::Value::String(event.event_type.clone()));
                    }
                    serde_json::to_string(&data)
                        .map(|json| {
                            Bytes::from(format!("event: {}\ndata: {}\n\n", event.event_type, json))
                        })
                        .map_err(io::Error::other)
                }
                Err(error) => Err(io::Error::other(error.to_string())),
            });
            let done = futures_util::stream::once(async {
                Ok::<Bytes, io::Error>(Bytes::from("data: [DONE]\n\n"))
            });
            let completion_streaming = StreamingResponse {
                stream: Box::pin(futures_util::stream::empty::<
                    Result<ResponsesEvent, unigateway_core::GatewayError>,
                >()),
                completion,
                request_id,
                request_metadata,
            };
            tokio::spawn(async move {
                let _ = completion_streaming.into_completion().await;
            });

            ProtocolHttpResponse::ok_sse(Box::pin(stream.chain(done)))
        }
    }
}

pub fn render_openai_responses_stream_from_completed(
    session: ProxySession<ResponsesEvent, ResponsesFinal>,
) -> ProtocolHttpResponse {
    match session {
        ProxySession::Completed(result) => {
            let raw = &result.response.raw;
            let response_id = raw
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(result.report.request_id.as_str())
                .to_string();
            let model = raw
                .get("model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let text = result.response.output_text.unwrap_or_default();
            let usage = raw
                .get("usage")
                .cloned()
                .unwrap_or_else(|| responses_usage_payload(result.report.usage.as_ref()));

            let mut chunks: Vec<Result<Bytes, io::Error>> = Vec::new();

            let created = serde_json::json!({
                "type": "response.created",
                "response": {
                    "id": response_id,
                    "object": "response",
                    "model": model,
                    "status": "in_progress"
                }
            });
            chunks.push(Ok(Bytes::from(format!(
                "event: response.created\ndata: {}\n\n",
                created
            ))));

            if !text.is_empty() {
                let delta = serde_json::json!({
                    "type": "response.output_text.delta",
                    "response_id": raw
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(result.report.request_id.as_str()),
                    "delta": text,
                });
                chunks.push(Ok(Bytes::from(format!(
                    "event: response.output_text.delta\ndata: {}\n\n",
                    delta
                ))));
            }

            let completed = serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": raw
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(result.report.request_id.as_str()),
                    "object": "response",
                    "model": raw
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                    "status": "completed",
                    "usage": usage,
                }
            });
            chunks.push(Ok(Bytes::from(format!(
                "event: response.completed\ndata: {}\n\n",
                completed
            ))));
            chunks.push(Ok(Bytes::from("data: [DONE]\n\n")));

            ProtocolHttpResponse::ok_sse(Box::pin(futures_util::stream::iter(chunks)))
        }
        ProxySession::Streaming(streaming) => {
            render_openai_responses_session(ProxySession::Streaming(streaming))
        }
    }
}

pub fn render_openai_embeddings_response(
    response: CompletedResponse<EmbeddingsResponse>,
) -> ProtocolHttpResponse {
    let raw = response.response.raw;
    let body = if raw.is_object() {
        raw
    } else {
        serde_json::json!({
            "object": "list",
            "data": [],
            "usage": response.report.usage.as_ref().map(|usage| serde_json::json!({
                "prompt_tokens": usage.input_tokens,
                "total_tokens": usage.total_tokens,
            })),
        })
    };

    ProtocolHttpResponse::ok_json(body)
}

pub fn anthropic_completed_chat_body(
    result: CompletedResponse<ChatResponseFinal>,
) -> serde_json::Value {
    if result.report.selected_provider == ProviderKind::Anthropic
        && result.response.raw.is_object()
        && result
            .response
            .raw
            .get("type")
            .and_then(serde_json::Value::as_str)
            == Some("message")
    {
        return result.response.raw;
    }

    let requested_model = anthropic_requested_model_alias_or(
        &result.report.metadata,
        result.response.model.as_deref().unwrap_or_default(),
    );

    if let Some(choice) = result
        .response
        .raw
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
    {
        let anthropic_id = result
            .response
            .raw
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|id| format!("msg_{id}"))
            .unwrap_or_else(|| result.report.request_id.clone());
        let content = choice
            .get("message")
            .map(openai_message_to_anthropic_content_blocks)
            .unwrap_or_default();
        let stop_reason = choice
            .get("finish_reason")
            .and_then(serde_json::Value::as_str)
            .map(map_finish_reason)
            .unwrap_or_else(|| {
                if content.iter().any(|block| {
                    block.get("type").and_then(serde_json::Value::as_str) == Some("tool_use")
                }) {
                    "tool_use".to_string()
                } else {
                    "end_turn".to_string()
                }
            });

        return serde_json::json!({
            "id": anthropic_id,
            "type": "message",
            "role": "assistant",
            "model": result.response.model.unwrap_or(requested_model),
            "content": content,
            "stop_reason": stop_reason,
            "stop_sequence": null,
            "usage": openai_usage_to_anthropic(result.response.raw.get("usage"))
                .unwrap_or_else(|| anthropic_usage_payload(result.report.usage.as_ref())),
        });
    }

    serde_json::json!({
        "id": result.report.request_id,
        "type": "message",
        "role": "assistant",
        "model": result.response.model.unwrap_or(requested_model),
        "content": [{
            "type": "text",
            "text": result.response.output_text.unwrap_or_default(),
        }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": anthropic_usage_payload(result.report.usage.as_ref()),
    })
}

pub fn openai_completed_chat_body(
    result: CompletedResponse<ChatResponseFinal>,
) -> serde_json::Value {
    if result.report.selected_provider == ProviderKind::OpenAiCompatible
        && result.response.raw.is_object()
        && result
            .response
            .raw
            .get("choices")
            .and_then(serde_json::Value::as_array)
            .is_some()
    {
        return result.response.raw;
    }

    serde_json::json!({
        "id": result.report.request_id,
        "object": "chat.completion",
        "model": result.response.model.unwrap_or_default(),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": result.response.output_text.unwrap_or_default(),
            },
            "finish_reason": "stop",
        }],
        "usage": result.report.usage.as_ref().map(|usage| serde_json::json!({
            "prompt_tokens": usage.input_tokens,
            "completion_tokens": usage.output_tokens,
            "total_tokens": usage.total_tokens,
        })),
    })
}

pub fn openai_sse_chunks_from_chat_chunk(
    request_id: &str,
    adapter: &mut OpenAiChatStreamAdapter,
    chunk: ChatResponseChunk,
) -> Vec<Bytes> {
    if chunk
        .raw
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .is_some()
    {
        return serde_json::to_string(&chunk.raw)
            .map(|json| vec![Bytes::from(format!("data: {json}\n\n"))])
            .unwrap_or_default();
    }

    let event_type = chunk
        .raw
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    match event_type {
        "message_start" => {
            adapter.model = chunk
                .raw
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    chunk
                        .raw
                        .get("message")
                        .and_then(|message| message.get("model"))
                        .and_then(serde_json::Value::as_str)
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
            let Some(delta) = chunk
                .raw
                .get("delta")
                .and_then(|delta| delta.get("text"))
                .and_then(serde_json::Value::as_str)
            else {
                return Vec::new();
            };

            if !adapter.sent_role_chunk {
                adapter.sent_role_chunk = true;
                return vec![
                    openai_chat_sse_bytes(
                        request_id,
                        adapter.model.as_deref().unwrap_or_default(),
                        serde_json::json!({"role": "assistant"}),
                        None,
                    ),
                    openai_chat_sse_bytes(
                        request_id,
                        adapter.model.as_deref().unwrap_or_default(),
                        serde_json::json!({"content": delta}),
                        None,
                    ),
                ];
            }

            vec![openai_chat_sse_bytes(
                request_id,
                adapter.model.as_deref().unwrap_or_default(),
                serde_json::json!({"content": delta}),
                None,
            )]
        }
        "message_stop" => vec![openai_chat_sse_bytes(
            request_id,
            adapter.model.as_deref().unwrap_or_default(),
            serde_json::json!({}),
            Some("stop"),
        )],
        _ => Vec::new(),
    }
}

fn openai_chat_sse_bytes(
    request_id: &str,
    model: &str,
    delta: serde_json::Value,
    finish_reason: Option<&str>,
) -> Bytes {
    let payload = serde_json::json!({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": 0,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason,
        }],
    });

    Bytes::from(format!("data: {}\n\n", payload))
}

async fn drive_anthropic_chat_stream(
    mut streaming: StreamingResponse<ChatResponseChunk, ChatResponseFinal>,
    requested_model: String,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
) {
    let request_id = streaming.request_id.clone();
    let mut state = AnthropicOpenAiStreamState::default();

    while let Some(item) = streaming.stream.next().await {
        match item {
            Ok(chunk) => {
                if is_native_anthropic_chunk(&chunk.raw) {
                    state.saw_native_anthropic = true;
                    let event_type = chunk
                        .raw
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("message_delta");
                    if emit_sse_json(&sender, event_type, chunk.raw.clone())
                        .await
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }

                if ensure_anthropic_stream_prelude(
                    &sender,
                    &request_id,
                    requested_model.as_str(),
                    &mut state,
                )
                .await
                .is_err()
                {
                    return;
                }

                if process_openai_chunk_for_anthropic_stream(&sender, &chunk.raw, &mut state)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                return;
            }
        }
    }

    let completion = match streaming.into_completion().await {
        Ok(completed) => completed,
        Err(error) => {
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
    };

    if state.saw_native_anthropic {
        return;
    }

    if ensure_anthropic_stream_prelude(&sender, &request_id, requested_model.as_str(), &mut state)
        .await
        .is_err()
    {
        return;
    }

    if state.next_content_index == 0
        && let Some(text) = completion
            .response
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
        && emit_text_delta(&sender, &mut state, text).await.is_err()
    {
        return;
    }

    if close_active_content_block(&sender, &mut state)
        .await
        .is_err()
    {
        return;
    }

    if flush_pending_tool_call_stops(&sender, &mut state)
        .await
        .is_err()
    {
        return;
    }

    let stop_reason = state.stop_reason.clone().unwrap_or_else(|| {
        if state.pending_tool_calls.is_empty() {
            "end_turn".to_string()
        } else {
            "tool_use".to_string()
        }
    });

    if emit_sse_json(
        &sender,
        "message_delta",
        serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null,
            },
            "usage": anthropic_usage_payload(completion.report.usage.as_ref()),
        }),
    )
    .await
    .is_err()
    {
        return;
    }

    let _ = emit_sse_json(
        &sender,
        "message_stop",
        serde_json::json!({
            "type": "message_stop",
        }),
    )
    .await;
}

fn anthropic_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
    })
}

#[derive(Default)]
struct AnthropicOpenAiStreamState {
    prelude_sent: bool,
    next_content_index: usize,
    active_content_block_index: Option<usize>,
    active_content_block_type: Option<&'static str>,
    stop_reason: Option<String>,
    pending_tool_calls: BTreeMap<usize, PendingOpenAiToolCall>,
    saw_native_anthropic: bool,
}

fn is_native_anthropic_chunk(raw: &serde_json::Value) -> bool {
    matches!(
        raw.get("type").and_then(serde_json::Value::as_str),
        Some(
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
                | "ping"
        )
    )
}

async fn ensure_anthropic_stream_prelude(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    request_id: &str,
    requested_model: &str,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    if state.prelude_sent {
        return Ok(());
    }

    let anthropic_id = format!("msg_{request_id}");

    emit_sse_json(
        sender,
        "message_start",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": anthropic_id,
                "type": "message",
                "role": "assistant",
                "model": requested_model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0,
                }
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;
    emit_sse_json(sender, "ping", serde_json::json!({ "type": "ping" }))
        .await
        .map_err(io::Error::other)?;
    state.prelude_sent = true;
    Ok(())
}

async fn emit_text_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<usize, io::Error> {
    emit_content_block_start(sender, state, "text").await
}

async fn emit_thinking_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<usize, io::Error> {
    emit_content_block_start(sender, state, "thinking").await
}

async fn emit_content_block_start(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    block_type: &'static str,
) -> Result<usize, io::Error> {
    if let Some(index) = state.active_content_block_index
        && state.active_content_block_type == Some(block_type)
    {
        return Ok(index);
    }

    close_active_content_block(sender, state).await?;

    let index = state.next_content_index;
    state.next_content_index += 1;

    let content_block = if block_type == "thinking" {
        serde_json::json!({
            "type": "thinking",
            "thinking": "",
            "signature": "",
        })
    } else {
        serde_json::json!({
            "type": "text",
            "text": "",
        })
    };

    emit_sse_json(
        sender,
        "content_block_start",
        serde_json::json!({
            "type": "content_block_start",
            "index": index,
            "content_block": content_block
        }),
    )
    .await
    .map_err(io::Error::other)?;
    state.active_content_block_index = Some(index);
    state.active_content_block_type = Some(block_type);
    Ok(index)
}

async fn emit_text_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    text: &str,
) -> Result<(), io::Error> {
    let index = emit_text_block_start(sender, state).await?;

    emit_sse_json(
        sender,
        "content_block_delta",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "text_delta",
                "text": text,
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn emit_thinking_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    thinking: &str,
) -> Result<(), io::Error> {
    let index = emit_thinking_block_start(sender, state).await?;

    emit_sse_json(
        sender,
        "content_block_delta",
        serde_json::json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "thinking_delta",
                "thinking": thinking,
            }
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn close_active_content_block(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let Some(index) = state.active_content_block_index.take() else {
        return Ok(());
    };

    let active_type = state.active_content_block_type.take();

    if active_type == Some("thinking") {
        emit_sse_json(
            sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {
                    "type": "signature_delta",
                    "signature": THINKING_SIGNATURE_PLACEHOLDER_VALUE,
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    emit_sse_json(
        sender,
        "content_block_stop",
        serde_json::json!({
            "type": "content_block_stop",
            "index": index,
        }),
    )
    .await
    .map_err(io::Error::other)?;

    Ok(())
}

async fn process_openai_chunk_for_anthropic_stream(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    raw: &serde_json::Value,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let choice = raw
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first());

    if let Some(finish_reason) = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(serde_json::Value::as_str)
        .filter(|reason| !reason.is_empty())
    {
        state.stop_reason = Some(map_finish_reason(finish_reason));
    }

    let delta = choice.and_then(|choice| choice.get("delta"));
    if let Some(thinking) = delta
        .and_then(|delta| {
            delta
                .get("reasoning_content")
                .or_else(|| delta.get("thinking"))
        })
        .and_then(serde_json::Value::as_str)
        .filter(|thinking| !thinking.is_empty())
    {
        emit_thinking_delta(sender, state, thinking).await?;
    }

    if let Some(text) = delta
        .and_then(|delta| delta.get("content"))
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
    {
        emit_text_delta(sender, state, text).await?;
    }

    if let Some(tool_calls) = delta
        .and_then(|delta| delta.get("tool_calls"))
        .and_then(serde_json::Value::as_array)
    {
        for (fallback_index, tool_call) in tool_calls.iter().enumerate() {
            let tool_index = tool_call
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(fallback_index);
            apply_openai_tool_call_delta(sender, state, tool_index, tool_call).await?;
        }
    }

    Ok(())
}

async fn apply_openai_tool_call_delta(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
    tool_index: usize,
    tool_call: &serde_json::Value,
) -> Result<(), io::Error> {
    let update = apply_openai_tool_call_delta_update(
        &mut state.pending_tool_calls,
        &mut state.next_content_index,
        tool_index,
        tool_call,
    );

    if update.start.is_some() {
        close_active_content_block(sender, state).await?;
    }

    if let Some(start) = update.start {
        emit_sse_json(
            sender,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": start.anthropic_index,
                "content_block": {
                    "type": "tool_use",
                    "id": start.id,
                    "name": start.name,
                    "input": {}
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    if let Some(delta) = update.delta {
        emit_sse_json(
            sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": delta.anthropic_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": delta.partial_json,
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    Ok(())
}

async fn flush_pending_tool_call_stops(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    state: &mut AnthropicOpenAiStreamState,
) -> Result<(), io::Error> {
    let tool_indexes = state.pending_tool_calls.keys().copied().collect::<Vec<_>>();
    for tool_index in tool_indexes {
        apply_openai_tool_call_delta(sender, state, tool_index, &serde_json::json!({})).await?;

        let update = flush_openai_tool_call_stop_update(&mut state.pending_tool_calls, tool_index);

        let Some(anthropic_index) = update.stop_index else {
            continue;
        };

        if let Some(start) = update.start {
            emit_sse_json(
                sender,
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start",
                    "index": start.anthropic_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": start.id,
                        "name": start.name,
                        "input": {}
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;
        }

        if let Some(delta) = update.delta {
            emit_sse_json(
                sender,
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": delta.anthropic_index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": delta.partial_json,
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;
        }

        emit_sse_json(
            sender,
            "content_block_stop",
            serde_json::json!({
                "type": "content_block_stop",
                "index": anthropic_index,
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    Ok(())
}

fn openai_usage_to_anthropic(usage: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let usage = usage?;
    let mut anthropic_usage = serde_json::json!({
        "input_tokens": usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "output_tokens": usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    });

    for key in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
        if let Some(value) = usage.get(key)
            && let Some(object) = anthropic_usage.as_object_mut()
        {
            object.insert(key.to_string(), value.clone());
        }
    }

    Some(anthropic_usage)
}

fn map_finish_reason(reason: &str) -> String {
    match reason {
        "stop" | "eos" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" | "function_call" | "tool_use" => "tool_use",
        _ => "end_turn",
    }
    .to_string()
}

fn responses_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
        "total_tokens": usage.and_then(|usage| usage.total_tokens).unwrap_or(0),
    })
}

async fn emit_sse_json(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    event_type: &str,
    data: serde_json::Value,
) -> Result<()> {
    let json = serde_json::to_string(&data)?;
    sender
        .send(Ok(Bytes::from(format!(
            "event: {event_type}\ndata: {json}\n\n"
        ))))
        .await
        .map_err(|_| anyhow!("anthropic downstream receiver dropped"))
}

#[cfg(test)]
mod tests {
    use super::AnthropicStreamAggregator;
    use serde_json::json;

    #[test]
    fn anthropic_stream_aggregator_rebuilds_thinking_signature_and_tool_use() {
        let mut aggregator = AnthropicStreamAggregator::default();

        aggregator
            .push_event(
                "message_start",
                &json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_123",
                        "type": "message",
                        "role": "assistant",
                        "model": "claude-opus-4-6",
                        "content": []
                    }
                }),
            )
            .expect("message_start");
        aggregator
            .push_event(
                "content_block_start",
                &json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "thinking",
                        "thinking": "",
                        "signature": ""
                    }
                }),
            )
            .expect("thinking start");
        aggregator
            .push_event(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "thinking_delta",
                        "thinking": "need weather first"
                    }
                }),
            )
            .expect("thinking delta");
        aggregator
            .push_event(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "signature_delta",
                        "signature": "real-signature"
                    }
                }),
            )
            .expect("signature delta");
        aggregator
            .push_event(
                "content_block_start",
                &json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "lookup_weather",
                        "input": {}
                    }
                }),
            )
            .expect("tool_use start");
        aggregator
            .push_event(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "{\"city\":"
                    }
                }),
            )
            .expect("tool_use delta 1");
        aggregator
            .push_event(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "\"Paris\"}"
                    }
                }),
            )
            .expect("tool_use delta 2");
        aggregator
            .push_event(
                "content_block_start",
                &json!({
                    "type": "content_block_start",
                    "index": 2,
                    "content_block": {
                        "type": "text",
                        "text": ""
                    }
                }),
            )
            .expect("text start");
        aggregator
            .push_event(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": 2,
                    "delta": {
                        "type": "text_delta",
                        "text": "Let me check."
                    }
                }),
            )
            .expect("text delta");
        aggregator
            .push_event(
                "message_delta",
                &json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": "tool_use",
                        "stop_sequence": null
                    },
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 5
                    }
                }),
            )
            .expect("message delta");
        aggregator
            .push_event("message_stop", &json!({"type": "message_stop"}))
            .expect("message stop");

        assert!(aggregator.is_complete());

        let message = aggregator.snapshot_message().expect("aggregated message");
        assert_eq!(
            message.get("role").and_then(serde_json::Value::as_str),
            Some("assistant")
        );
        assert_eq!(
            message
                .get("stop_reason")
                .and_then(serde_json::Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            message
                .pointer("/content/0/type")
                .and_then(serde_json::Value::as_str),
            Some("thinking")
        );
        assert_eq!(
            message
                .pointer("/content/0/signature")
                .and_then(serde_json::Value::as_str),
            Some("real-signature")
        );
        assert_eq!(
            message
                .pointer("/content/1/type")
                .and_then(serde_json::Value::as_str),
            Some("tool_use")
        );
        assert_eq!(
            message
                .pointer("/content/1/input/city")
                .and_then(serde_json::Value::as_str),
            Some("Paris")
        );
        assert_eq!(
            message
                .pointer("/content/2/text")
                .and_then(serde_json::Value::as_str),
            Some("Let me check.")
        );
    }

    #[test]
    fn anthropic_stream_aggregator_push_chunk_uses_chunk_type() {
        let mut aggregator = AnthropicStreamAggregator::default();
        let chunk = unigateway_core::ChatResponseChunk {
            delta: None,
            raw: json!({
                "type": "message_start",
                "message": {
                    "id": "msg_456",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-sonnet-4-5",
                    "content": []
                }
            }),
        };

        aggregator.push_chunk(&chunk).expect("push chunk");

        let message = aggregator.snapshot_message().expect("snapshot");
        assert_eq!(
            message.get("id").and_then(serde_json::Value::as_str),
            Some("msg_456")
        );
        assert_eq!(
            message.get("model").and_then(serde_json::Value::as_str),
            Some("claude-sonnet-4-5")
        );
    }
}
