use std::io;

use anyhow::{Result, anyhow};
use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{ChatRequest, Message as ConnectorMessage, ResponsesRequest, Role};
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, Message,
    MessageRole, ProviderKind, ProxyChatRequest, ProxyResponsesRequest, ProxySession,
    ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};

pub(super) fn to_core_chat_request(request: &ChatRequest) -> ProxyChatRequest {
    ProxyChatRequest {
        model: request.model.clone(),
        messages: request.messages.iter().map(to_core_message).collect(),
        temperature: request.temperature,
        top_p: request.top_p,
        max_tokens: request.max_tokens,
        stream: request.stream.unwrap_or(false),
        metadata: std::collections::HashMap::new(),
    }
}

pub(super) fn to_core_responses_request(request: &ResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        model: request.model.clone(),
        input: request.input.clone(),
        instructions: request.instructions.clone(),
        temperature: request.temperature,
        top_p: request.top_p,
        max_output_tokens: request.max_output_tokens,
        stream: request.stream.unwrap_or(false),
        tools: request.tools.clone(),
        tool_choice: request.tool_choice.clone(),
        previous_response_id: request.previous_response_id.clone(),
        request_metadata: request.metadata.clone(),
        extra: filtered_response_extra(request),
        metadata: std::collections::HashMap::new(),
    }
}

fn filtered_response_extra(
    request: &ResponsesRequest,
) -> std::collections::HashMap<String, serde_json::Value> {
    request
        .extra
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "target_vendor" | "target_provider" | "provider"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn to_core_message(message: &ConnectorMessage) -> Message {
    Message {
        role: match message.role {
            Role::System => MessageRole::System,
            Role::Assistant => MessageRole::Assistant,
            Role::Tool => MessageRole::Tool,
            _ => MessageRole::User,
        },
        content: message.content_as_text(),
    }
}

pub(super) fn chat_session_to_openai_response(
    session: ProxySession<unigateway_core::ChatResponseChunk, unigateway_core::ChatResponseFinal>,
) -> Response {
    match session {
        ProxySession::Completed(result) => {
            let raw = result.response.raw;
            let body = if raw.is_object() {
                raw
            } else {
                serde_json::json!({
                    "id": result.report.request_id,
                    "object": "chat.completion",
                    "model": result.response.model,
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": result.response.output_text,
                        },
                        "finish_reason": "stop",
                    }],
                    "usage": result.report.usage.as_ref().map(|usage| serde_json::json!({
                        "prompt_tokens": usage.input_tokens,
                        "completion_tokens": usage.output_tokens,
                        "total_tokens": usage.total_tokens,
                    })),
                })
            };
            (StatusCode::OK, Json(body)).into_response()
        }
        ProxySession::Streaming(streaming) => {
            let stream = streaming.stream.map(|item| match item {
                Ok(chunk) => serde_json::to_string(&chunk.raw)
                    .map(|json| Bytes::from(format!("data: {json}\n\n")))
                    .map_err(io::Error::other),
                Err(error) => Err(io::Error::other(error.to_string())),
            });
            let done = futures_util::stream::once(async {
                Ok::<Bytes, io::Error>(Bytes::from("data: [DONE]\n\n"))
            });
            let completion = streaming.completion;
            tokio::spawn(async move {
                let _ = completion.await;
            });

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream.chain(done)),
            )
                .into_response()
        }
    }
}

pub(super) fn chat_session_to_anthropic_response(
    session: ProxySession<ChatResponseChunk, ChatResponseFinal>,
    requested_model: String,
) -> Response {
    match session {
        ProxySession::Completed(result) => {
            let body = anthropic_completed_chat_body(result, &requested_model);
            (StatusCode::OK, Json(body)).into_response()
        }
        ProxySession::Streaming(streaming) => {
            let (sender, receiver) = mpsc::channel(16);
            tokio::spawn(async move {
                drive_anthropic_chat_stream(streaming, requested_model, sender).await;
            });

            let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|item| (item, receiver))
            });

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream),
            )
                .into_response()
        }
    }
}

pub(super) fn responses_session_to_openai_response(
    session: ProxySession<ResponsesEvent, ResponsesFinal>,
) -> Response {
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
            (StatusCode::OK, Json(body)).into_response()
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

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream.chain(done)),
            )
                .into_response()
        }
    }
}

pub(super) fn embeddings_response_to_openai_response(
    response: CompletedResponse<EmbeddingsResponse>,
) -> Response {
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

    (StatusCode::OK, Json(body)).into_response()
}

fn anthropic_completed_chat_body(
    result: CompletedResponse<ChatResponseFinal>,
    requested_model: &str,
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

    serde_json::json!({
        "id": result.report.request_id,
        "type": "message",
        "role": "assistant",
        "model": result.response.model.unwrap_or_else(|| requested_model.to_string()),
        "content": [{
            "type": "text",
            "text": result.response.output_text.unwrap_or_default(),
        }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": anthropic_usage_payload(result.report.usage.as_ref()),
    })
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
    use std::collections::HashMap;

    use llm_connector::types::ResponsesRequest;
    use unigateway_core::{ChatResponseFinal, CompletedResponse, ProviderKind, RequestReport};

    use super::{anthropic_completed_chat_body, filtered_response_extra};

    #[test]
    fn responses_extra_filter_strips_gateway_routing_hints_only() {
        let filtered = filtered_response_extra(&ResponsesRequest {
            model: "gpt-4.1-mini".to_string(),
            extra: HashMap::from([
                (
                    "reasoning".to_string(),
                    serde_json::json!({"effort": "high"}),
                ),
                ("target_provider".to_string(), serde_json::json!("deepseek")),
                ("provider".to_string(), serde_json::json!("moonshot")),
            ]),
            ..ResponsesRequest::default()
        });

        assert!(filtered.contains_key("reasoning"));
        assert!(!filtered.contains_key("target_provider"));
        assert!(!filtered.contains_key("provider"));
    }

    #[test]
    fn anthropic_completed_body_normalizes_openai_provider_output() {
        let body = anthropic_completed_chat_body(
            CompletedResponse {
                response: ChatResponseFinal {
                    model: Some("glm-4.5".to_string()),
                    output_text: Some("pong".to_string()),
                    raw: serde_json::json!({
                        "id": "chatcmpl_123",
                        "object": "chat.completion",
                    }),
                },
                report: RequestReport {
                    request_id: "req_123".to_string(),
                    pool_id: Some("svc".to_string()),
                    selected_endpoint_id: "zhipu-main".to_string(),
                    selected_provider: ProviderKind::OpenAiCompatible,
                    attempts: Vec::new(),
                    usage: Some(unigateway_core::TokenUsage {
                        input_tokens: Some(10),
                        output_tokens: Some(4),
                        total_tokens: Some(14),
                    }),
                    latency_ms: 12,
                    started_at: std::time::SystemTime::UNIX_EPOCH,
                    finished_at: std::time::SystemTime::UNIX_EPOCH,
                    metadata: HashMap::new(),
                },
            },
            "claude-3-5-sonnet-latest",
        );

        assert_eq!(
            body.get("type").and_then(serde_json::Value::as_str),
            Some("message")
        );
        assert_eq!(
            body.get("content")
                .and_then(serde_json::Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("text"))
                .and_then(serde_json::Value::as_str),
            Some("pong")
        );
        assert_eq!(
            body.get("usage")
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(serde_json::Value::as_u64),
            Some(10)
        );
    }
}
