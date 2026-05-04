use std::io;

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProviderKind,
    ProxySession, ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
    conversion::openai_message_to_anthropic_content_blocks,
};

use crate::{ProtocolHttpResponse, anthropic_requested_model_alias_or};

use super::anthropic_stream::{
    anthropic_usage_payload, drive_anthropic_chat_stream, map_finish_reason,
};
use super::openai_chat::{OpenAiChatStreamAdapter, openai_sse_chunks_from_chat_chunk};

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

                stream::iter(chunks)
            });
            let done =
                stream::once(async { Ok::<Bytes, io::Error>(Bytes::from("data: [DONE]\n\n")) });
            let completion_streaming = StreamingResponse {
                stream: Box::pin(stream::empty::<
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

            let stream = stream::unfold(receiver, |mut receiver| async move {
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
            let done =
                stream::once(async { Ok::<Bytes, io::Error>(Bytes::from("data: [DONE]\n\n")) });
            let completion_streaming = StreamingResponse {
                stream: Box::pin(stream::empty::<
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

            ProtocolHttpResponse::ok_sse(Box::pin(stream::iter(chunks)))
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

fn responses_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
        "total_tokens": usage.and_then(|usage| usage.total_tokens).unwrap_or(0),
    })
}
