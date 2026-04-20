use std::collections::BTreeMap;
use std::io;

use anyhow::{Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProviderKind,
    ProxySession, ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};

use crate::{ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY, ProtocolHttpResponse};

const EXTENDED_THINKING_PLACEHOLDER_SIG: &str = "EXTENDED_THINKING_PLACEHOLDER_SIG";

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
            let request_id = streaming.request_id.clone();
            let adapter_state =
                std::sync::Arc::new(std::sync::Mutex::new(OpenAiChatStreamAdapter::default()));

            let stream = streaming.stream.flat_map(move |item| {
                let request_id = request_id.clone();
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
            let completion = streaming.completion;
            tokio::spawn(async move {
                let _ = completion.await;
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
            let requested_model = requested_model_alias_from_metadata(
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
            let stream = streaming.stream.map(|item| match item {
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
            let completion = streaming.completion;
            tokio::spawn(async move {
                let _ = completion.await;
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

    let requested_model = requested_model_alias_from_metadata(
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

    let completion = match streaming.completion.await {
        Ok(Ok(completed)) => completed,
        Ok(Err(error)) => {
            let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
            return;
        }
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
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
    emitted_argument_len: usize,
    anthropic_index: Option<usize>,
    started: bool,
    stopped: bool,
}

#[derive(Default)]
struct AnthropicOpenAiStreamState {
    prelude_sent: bool,
    next_content_index: usize,
    active_content_block_index: Option<usize>,
    active_content_block_type: Option<&'static str>,
    stop_reason: Option<String>,
    pending_tool_calls: BTreeMap<usize, PendingToolCall>,
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
                    "signature": EXTENDED_THINKING_PLACEHOLDER_SIG,
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
    state.pending_tool_calls.entry(tool_index).or_default();

    if state
        .pending_tool_calls
        .get(&tool_index)
        .and_then(|pending| pending.anthropic_index)
        .is_none()
    {
        let anthropic_index = state.next_content_index;
        state.next_content_index += 1;
        if let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) {
            pending.anthropic_index = Some(anthropic_index);
        }
    }

    if let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) {
        if let Some(id) = tool_call.get("id").and_then(serde_json::Value::as_str) {
            pending.id = id.to_string();
        }
        if let Some(name) = tool_call
            .get("function")
            .and_then(|value| value.get("name"))
            .and_then(serde_json::Value::as_str)
        {
            pending.name = name.to_string();
        }
        if let Some(arguments) = tool_call
            .get("function")
            .and_then(|value| value.get("arguments"))
            .and_then(serde_json::Value::as_str)
        {
            pending.arguments.push_str(arguments);
        }
    }

    let mut start_payload = None;
    let mut delta_payload = None;
    if let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) {
        let can_start = !pending.started && !pending.id.is_empty() && !pending.name.is_empty();
        if can_start {
            pending.started = true;
            start_payload = Some((
                pending.anthropic_index.unwrap_or(tool_index),
                pending.id.clone(),
                pending.name.clone(),
            ));
        }

        if pending.started && pending.emitted_argument_len < pending.arguments.len() {
            let fragment = pending.arguments[pending.emitted_argument_len..].to_string();
            pending.emitted_argument_len = pending.arguments.len();
            delta_payload = Some((pending.anthropic_index.unwrap_or(tool_index), fragment));
        }
    }

    if start_payload.is_some() {
        close_active_content_block(sender, state).await?;
    }

    if let Some((anthropic_index, id, name)) = start_payload {
        emit_sse_json(
            sender,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": anthropic_index,
                "content_block": {
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": {}
                }
            }),
        )
        .await
        .map_err(io::Error::other)?;
    }

    if let Some((anthropic_index, fragment)) = delta_payload {
        emit_sse_json(
            sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": anthropic_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": fragment,
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

        let stop_index = {
            let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) else {
                continue;
            };
            if !pending.started {
                pending.started = true;
                if pending.id.is_empty() && pending.name.is_empty() && pending.arguments.is_empty()
                {
                    None
                } else {
                    Some((
                        pending.anthropic_index.unwrap_or(tool_index),
                        pending.id.clone(),
                        pending.name.clone(),
                        pending.emitted_argument_len < pending.arguments.len(),
                        pending.arguments[pending.emitted_argument_len..].to_string(),
                    ))
                }
            } else if pending.stopped {
                None
            } else {
                Some((
                    pending.anthropic_index.unwrap_or(tool_index),
                    String::new(),
                    String::new(),
                    false,
                    String::new(),
                ))
            }
        };

        let Some((anthropic_index, id, name, emit_buffered_delta, buffered_delta)) = stop_index
        else {
            continue;
        };

        if !id.is_empty() || !name.is_empty() || emit_buffered_delta {
            emit_sse_json(
                sender,
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start",
                    "index": anthropic_index,
                    "content_block": {
                        "type": "tool_use",
                        "id": if id.is_empty() { "toolu_unknown" } else { id.as_str() },
                        "name": if name.is_empty() { "tool" } else { name.as_str() },
                        "input": {}
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;
        }

        if emit_buffered_delta {
            emit_sse_json(
                sender,
                "content_block_delta",
                serde_json::json!({
                    "type": "content_block_delta",
                    "index": anthropic_index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": buffered_delta,
                    }
                }),
            )
            .await
            .map_err(io::Error::other)?;

            if let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) {
                pending.emitted_argument_len = pending.arguments.len();
            }
        }

        if let Some(pending) = state.pending_tool_calls.get_mut(&tool_index) {
            if pending.stopped {
                continue;
            }
            pending.stopped = true;
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

fn openai_message_to_anthropic_content_blocks(
    message: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut content_blocks = Vec::new();

    if let Some(thinking) = message
        .get("reasoning_content")
        .or_else(|| message.get("thinking"))
        .and_then(serde_json::Value::as_str)
    {
        content_blocks.push(serde_json::json!({
            "type": "thinking",
            "thinking": thinking,
            "signature": EXTENDED_THINKING_PLACEHOLDER_SIG,
        }));
    }

    match message.get("content") {
        Some(serde_json::Value::String(text)) if !text.is_empty() => {
            content_blocks.push(serde_json::json!({
                "type": "text",
                "text": text,
            }));
        }
        Some(serde_json::Value::Array(blocks)) => {
            content_blocks.extend(blocks.iter().filter_map(|block| {
                if block.get("type").and_then(serde_json::Value::as_str) == Some("text") {
                    Some(block.clone())
                } else {
                    None
                }
            }));
        }
        _ => {}
    }

    if let Some(tool_calls) = message
        .get("tool_calls")
        .and_then(serde_json::Value::as_array)
    {
        content_blocks.extend(
            tool_calls
                .iter()
                .filter_map(openai_tool_call_to_anthropic_block),
        );
    }

    content_blocks
}

fn openai_tool_call_to_anthropic_block(tool_call: &serde_json::Value) -> Option<serde_json::Value> {
    let function = tool_call.get("function")?;
    let arguments = function
        .get("arguments")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("{}");
    let parsed_input = serde_json::from_str::<serde_json::Value>(arguments)
        .unwrap_or_else(|_| serde_json::json!({}));

    Some(serde_json::json!({
        "type": "tool_use",
        "id": tool_call
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("toolu_unknown"),
        "name": function
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool"),
        "input": parsed_input,
    }))
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

fn requested_model_alias_from_metadata(
    metadata: &std::collections::HashMap<String, String>,
    fallback: &str,
) -> String {
    metadata
        .get(ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY)
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}
