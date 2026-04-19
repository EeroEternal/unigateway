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
    let mut content_block_started = false;
    let mut buffered_text = String::new();

    if emit_sse_json(
        &sender,
        "message_start",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": request_id,
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
    .is_err()
    {
        return;
    }

    while let Some(item) = streaming.stream.next().await {
        match item {
            Ok(chunk) => {
                if let Some(delta) = chunk.delta.filter(|delta| !delta.is_empty()) {
                    if !content_block_started {
                        if emit_sse_json(
                            &sender,
                            "content_block_start",
                            serde_json::json!({
                                "type": "content_block_start",
                                "index": 0,
                                "content_block": {
                                    "type": "text",
                                    "text": "",
                                }
                            }),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        content_block_started = true;
                    }

                    buffered_text.push_str(&delta);
                    if emit_sse_json(
                        &sender,
                        "content_block_delta",
                        serde_json::json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": {
                                "type": "text_delta",
                                "text": delta,
                            }
                        }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }
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

    if !content_block_started
        && let Some(text) = completion
            .response
            .output_text
            .as_deref()
            .filter(|text| !text.is_empty())
    {
        if emit_sse_json(
            &sender,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "text",
                    "text": "",
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }
        if emit_sse_json(
            &sender,
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "text_delta",
                    "text": text,
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }
        buffered_text.push_str(text);
        content_block_started = true;
    }

    if content_block_started
        && emit_sse_json(
            &sender,
            "content_block_stop",
            serde_json::json!({
                "type": "content_block_stop",
                "index": 0,
            }),
        )
        .await
        .is_err()
    {
        return;
    }

    if emit_sse_json(
        &sender,
        "message_delta",
        serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn",
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
