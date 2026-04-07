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
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, Endpoint,
    EndpointRef, ExecutionPlan, ExecutionTarget, GatewayError, LoadBalancingStrategy, ModelPolicy,
    ProviderKind, ProviderPool, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
    ProxySession, ResponsesEvent, ResponsesFinal, RetryPolicy, SecretString, StreamingResponse,
    TokenUsage,
};

use crate::host::RuntimeContext;

pub async fn try_anthropic_chat_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: ProxyChatRequest,
    requested_model: &str,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = runtime
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_anthropic_response(
        session,
        requested_model.to_string(),
    )))
}

pub async fn try_openai_chat_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_openai_chat_via_core(runtime, pool, hint, request).await
}

pub async fn try_openai_chat_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: ProxyChatRequest,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    runtime
        .core_engine()
        .upsert_pool(pool.clone())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    execute_openai_chat_via_core(runtime, pool, hint, request).await
}

pub async fn try_openai_responses_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_openai_responses_via_core(runtime, pool, hint, request).await
}

pub async fn try_openai_responses_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    runtime
        .core_engine()
        .upsert_pool(pool.clone())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    execute_openai_responses_via_core(runtime, pool, hint, request).await
}

pub async fn try_anthropic_chat_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: ProxyChatRequest,
    requested_model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_anthropic_pool(runtime.config.anthropic_model, base_url, api_key);

    runtime
        .core_engine()
        .upsert_pool(pool.clone())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    execute_anthropic_chat_via_core(runtime, pool, hint, request, requested_model).await
}

pub async fn try_openai_embeddings_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => {
            return Err(anyhow!(
                "no provider pool available for service '{service_id}'"
            ));
        }
    };

    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;
    let response = runtime
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

pub async fn try_openai_embeddings_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    runtime
        .core_engine()
        .upsert_pool(pool.clone())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;
    let response = runtime
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

fn chat_session_to_openai_response(
    session: ProxySession<ChatResponseChunk, ChatResponseFinal>,
) -> Response {
    match session {
        ProxySession::Completed(result) => {
            let body = openai_completed_chat_body(result);
            (StatusCode::OK, Json(body)).into_response()
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

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(stream.chain(done)),
            )
                .into_response()
        }
    }
}

fn chat_session_to_anthropic_response(
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

fn responses_session_to_openai_response(
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

fn build_responses_stream_response_from_completed(
    session: ProxySession<ResponsesEvent, ResponsesFinal>,
) -> Response {
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

            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from_stream(futures_util::stream::iter(chunks)),
            )
                .into_response()
        }
        ProxySession::Streaming(streaming) => {
            responses_session_to_openai_response(ProxySession::Streaming(streaming))
        }
    }
}

fn embeddings_response_to_openai_response(
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

fn openai_completed_chat_body(result: CompletedResponse<ChatResponseFinal>) -> serde_json::Value {
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

#[derive(Default)]
struct OpenAiChatStreamAdapter {
    model: Option<String>,
    sent_role_chunk: bool,
}

fn openai_sse_chunks_from_chat_chunk(
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

async fn execute_openai_chat_via_core(
    runtime: &RuntimeContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> Result<Option<Response>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = runtime
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_openai_response(session)))
}

async fn execute_anthropic_chat_via_core(
    runtime: &RuntimeContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
    requested_model: &str,
) -> Result<Option<Response>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = runtime
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_anthropic_response(
        session,
        requested_model.to_string(),
    )))
}

async fn execute_openai_responses_via_core(
    runtime: &RuntimeContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> Result<Option<Response>> {
    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;

    let response = match execute_openai_responses_with_compat(
        runtime,
        target.clone(),
        request.clone(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) if should_retry_responses_without_tools(&request) => {
            execute_openai_responses_with_compat(runtime, target, without_response_tools(request))
                .await
                .map_err(|retry_error| anyhow!(retry_error.to_string()))?
        }
        Err(error) => return Err(anyhow!(error.to_string())),
    };

    Ok(Some(response))
}

async fn execute_openai_responses_with_compat(
    runtime: &RuntimeContext<'_>,
    target: ExecutionTarget,
    request: ProxyResponsesRequest,
) -> Result<Response, GatewayError> {
    if request.stream {
        match runtime
            .core_engine()
            .proxy_responses(request.clone(), target.clone())
            .await
        {
            Ok(session) => return Ok(responses_session_to_openai_response(session)),
            Err(stream_error) => {
                let mut fallback_request = request;
                fallback_request.stream = false;

                return runtime
                    .core_engine()
                    .proxy_responses(fallback_request, target)
                    .await
                    .map(build_responses_stream_response_from_completed)
                    .map_err(|fallback_error| {
                        if should_preserve_stream_error(&stream_error, &fallback_error) {
                            stream_error
                        } else {
                            fallback_error
                        }
                    });
            }
        }
    }

    runtime
        .core_engine()
        .proxy_responses(request, target)
        .await
        .map(responses_session_to_openai_response)
}

async fn prepare_core_pool(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
) -> Result<Option<ProviderPool>> {
    runtime.pool_for_service(service_id).await
}

fn build_execution_target(
    endpoints: &[Endpoint],
    pool_id: &str,
    hint: Option<&str>,
) -> Result<ExecutionTarget> {
    let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
        return Ok(ExecutionTarget::Pool {
            pool_id: pool_id.to_string(),
        });
    };

    let candidates: Vec<EndpointRef> = endpoints
        .iter()
        .filter(|endpoint| endpoint_matches_hint(endpoint, hint))
        .map(|endpoint| EndpointRef {
            endpoint_id: endpoint.endpoint_id.clone(),
        })
        .collect();

    if candidates.is_empty() {
        return Err(anyhow!("no provider matches target '{hint}'"));
    }

    Ok(ExecutionTarget::Plan(ExecutionPlan {
        pool_id: Some(pool_id.to_string()),
        candidates,
        load_balancing_override: None,
        retry_policy_override: None,
        metadata: std::collections::HashMap::new(),
    }))
}

fn build_openai_compatible_target(
    endpoints: &[Endpoint],
    pool_id: &str,
    hint: Option<&str>,
) -> Result<ExecutionTarget> {
    let compatible_endpoints: Vec<&Endpoint> = endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .filter(|endpoint| endpoint.provider_kind == ProviderKind::OpenAiCompatible)
        .collect();

    if compatible_endpoints.is_empty() {
        return Err(anyhow!("no openai-compatible provider available"));
    }

    let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
        let enabled_count = endpoints.iter().filter(|endpoint| endpoint.enabled).count();
        if compatible_endpoints.len() == enabled_count {
            return Ok(ExecutionTarget::Pool {
                pool_id: pool_id.to_string(),
            });
        }

        return Ok(ExecutionTarget::Plan(ExecutionPlan {
            pool_id: Some(pool_id.to_string()),
            candidates: compatible_endpoints
                .into_iter()
                .map(|endpoint| EndpointRef {
                    endpoint_id: endpoint.endpoint_id.clone(),
                })
                .collect(),
            load_balancing_override: None,
            retry_policy_override: None,
            metadata: std::collections::HashMap::new(),
        }));
    };

    let candidates: Vec<EndpointRef> = compatible_endpoints
        .into_iter()
        .filter(|endpoint| endpoint_matches_hint(endpoint, hint))
        .map(|endpoint| EndpointRef {
            endpoint_id: endpoint.endpoint_id.clone(),
        })
        .collect();

    if candidates.is_empty() {
        return Err(anyhow!("no provider matches target '{hint}'"));
    }

    Ok(ExecutionTarget::Plan(ExecutionPlan {
        pool_id: Some(pool_id.to_string()),
        candidates,
        load_balancing_override: None,
        retry_policy_override: None,
        metadata: std::collections::HashMap::new(),
    }))
}

fn endpoint_matches_hint(endpoint: &Endpoint, hint: &str) -> bool {
    endpoint.endpoint_id.eq_ignore_ascii_case(hint)
        || endpoint
            .metadata
            .get("provider_name")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .metadata
            .get("source_endpoint_id")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .metadata
            .get("provider_family")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
}

fn build_env_openai_pool(default_model: &str, base_url: &str, api_key: &str) -> ProviderPool {
    ProviderPool {
        pool_id: "__env_openai__".to_string(),
        endpoints: vec![Endpoint {
            endpoint_id: "env-openai".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: normalize_base_url(base_url),
            api_key: SecretString::new(api_key),
            model_policy: ModelPolicy {
                default_model: Some(default_model.to_string()),
                model_mapping: std::collections::HashMap::new(),
            },
            enabled: true,
            metadata: std::collections::HashMap::from([
                ("provider_name".to_string(), "env-openai".to_string()),
                ("source_endpoint_id".to_string(), "env-openai".to_string()),
                ("provider_family".to_string(), "openai".to_string()),
            ]),
        }],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: std::collections::HashMap::from([(
            "service_name".to_string(),
            "env-openai".to_string(),
        )]),
    }
}

fn build_env_anthropic_pool(default_model: &str, base_url: &str, api_key: &str) -> ProviderPool {
    ProviderPool {
        pool_id: "__env_anthropic__".to_string(),
        endpoints: vec![Endpoint {
            endpoint_id: "env-anthropic".to_string(),
            provider_kind: ProviderKind::Anthropic,
            driver_id: "anthropic".to_string(),
            base_url: normalize_base_url(base_url),
            api_key: SecretString::new(api_key),
            model_policy: ModelPolicy {
                default_model: Some(default_model.to_string()),
                model_mapping: std::collections::HashMap::new(),
            },
            enabled: true,
            metadata: std::collections::HashMap::from([
                ("provider_name".to_string(), "env-anthropic".to_string()),
                (
                    "source_endpoint_id".to_string(),
                    "env-anthropic".to_string(),
                ),
                ("provider_family".to_string(), "anthropic".to_string()),
            ]),
        }],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: std::collections::HashMap::from([(
            "service_name".to_string(),
            "env-anthropic".to_string(),
        )]),
    }
}

fn normalize_base_url(url: &str) -> String {
    let mut normalized = url.trim().to_string();
    if normalized.is_empty() {
        return normalized;
    }
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn responses_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
        "total_tokens": usage.and_then(|usage| usage.total_tokens).unwrap_or(0),
    })
}

fn without_response_tools(request: ProxyResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        tools: None,
        tool_choice: None,
        ..request
    }
}

fn should_retry_responses_without_tools(request: &ProxyResponsesRequest) -> bool {
    request.tools.is_some() || request.tool_choice.is_some()
}

fn should_preserve_stream_error(
    stream_error: &GatewayError,
    fallback_error: &GatewayError,
) -> bool {
    matches!(
        stream_error.terminal_error(),
        GatewayError::InvalidRequest(_)
            | GatewayError::PoolNotFound(_)
            | GatewayError::EndpointNotFound(_)
    ) || matches!(
        fallback_error.terminal_error(),
        GatewayError::InvalidRequest(_)
            | GatewayError::PoolNotFound(_)
            | GatewayError::EndpointNotFound(_)
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{
        ChatResponseChunk, ChatResponseFinal, CompletedResponse, Endpoint, EndpointRef,
        ExecutionPlan, ExecutionTarget, GatewayError, ModelPolicy, ProviderKind,
        ProxyResponsesRequest, RequestReport, SecretString,
    };

    use super::{
        OpenAiChatStreamAdapter, anthropic_completed_chat_body, build_env_anthropic_pool,
        build_env_openai_pool, build_openai_compatible_target, endpoint_matches_hint,
        openai_completed_chat_body, openai_sse_chunks_from_chat_chunk,
        should_preserve_stream_error, without_response_tools,
    };

    fn endpoint() -> Endpoint {
        Endpoint {
            endpoint_id: "deepseek-main".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: "https://api.example.com".to_string(),
            api_key: SecretString::new("sk-test"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::from([
                ("provider_name".to_string(), "DeepSeek-Main".to_string()),
                (
                    "source_endpoint_id".to_string(),
                    "deepseek:global".to_string(),
                ),
                ("provider_family".to_string(), "deepseek".to_string()),
            ]),
        }
    }

    #[test]
    fn endpoint_hint_matching_supports_existing_product_forms() {
        let endpoint = endpoint();
        assert!(endpoint_matches_hint(&endpoint, "deepseek-main"));
        assert!(endpoint_matches_hint(&endpoint, "DeepSeek-Main"));
        assert!(endpoint_matches_hint(&endpoint, "deepseek:global"));
        assert!(endpoint_matches_hint(&endpoint, "deepseek"));
        assert!(!endpoint_matches_hint(&endpoint, "zhipu"));
    }

    #[test]
    fn env_openai_pool_matches_basic_openai_hints() {
        let pool = build_env_openai_pool("gpt-4o-mini", "https://api.openai.com", "sk-test");
        let endpoint = pool.endpoints.first().expect("endpoint");

        assert!(endpoint_matches_hint(endpoint, "env-openai"));
        assert!(endpoint_matches_hint(endpoint, "openai"));
        assert!(!endpoint_matches_hint(endpoint, "deepseek"));
    }

    #[test]
    fn env_anthropic_pool_matches_basic_anthropic_hints() {
        let pool =
            build_env_anthropic_pool("claude-3-5-sonnet", "https://api.anthropic.com", "sk-ant");
        let endpoint = pool.endpoints.first().expect("endpoint");

        assert!(endpoint_matches_hint(endpoint, "env-anthropic"));
        assert!(endpoint_matches_hint(endpoint, "anthropic"));
        assert!(!endpoint_matches_hint(endpoint, "openai"));
    }

    #[test]
    fn responses_tool_stripping_clears_tool_fields_only() {
        let request = without_response_tools(ProxyResponsesRequest {
            model: "gpt-4.1-mini".to_string(),
            input: Some(serde_json::json!("hello")),
            instructions: Some("be terse".to_string()),
            temperature: Some(0.1),
            top_p: Some(0.8),
            max_output_tokens: Some(128),
            stream: true,
            tools: Some(serde_json::json!([])),
            tool_choice: Some(serde_json::json!("auto")),
            previous_response_id: Some("resp_prev".to_string()),
            request_metadata: Some(serde_json::json!({"trace_id": "abc"})),
            extra: std::collections::HashMap::new(),
            metadata: HashMap::new(),
        });

        assert!(request.tools.is_none());
        assert!(request.tool_choice.is_none());
        assert_eq!(request.instructions.as_deref(), Some("be terse"));
        assert_eq!(request.previous_response_id.as_deref(), Some("resp_prev"));
    }

    #[test]
    fn stream_error_preservation_prefers_routing_failures() {
        assert!(should_preserve_stream_error(
            &GatewayError::InvalidRequest("bad target".to_string()),
            &GatewayError::UpstreamHttp {
                status: 500,
                body: Some("boom".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
        assert!(should_preserve_stream_error(
            &GatewayError::Transport {
                message: "stream failed".to_string(),
                endpoint_id: Some("ep-1".to_string()),
            },
            &GatewayError::PoolNotFound("svc".to_string()),
        ));
        assert!(!should_preserve_stream_error(
            &GatewayError::Transport {
                message: "stream failed".to_string(),
                endpoint_id: Some("ep-1".to_string()),
            },
            &GatewayError::UpstreamHttp {
                status: 500,
                body: Some("boom".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
    }

    #[test]
    fn openai_compatible_target_filters_mixed_pool() {
        let anthropic_endpoint = Endpoint {
            endpoint_id: "anthropic-main".to_string(),
            provider_kind: ProviderKind::Anthropic,
            driver_id: "anthropic".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            api_key: SecretString::new("sk-ant"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::new(),
        };

        let target =
            build_openai_compatible_target(&[endpoint(), anthropic_endpoint], "pool-1", None)
                .expect("target");

        assert_eq!(
            target,
            ExecutionTarget::Plan(ExecutionPlan {
                pool_id: Some("pool-1".to_string()),
                candidates: vec![EndpointRef {
                    endpoint_id: "deepseek-main".to_string(),
                }],
                load_balancing_override: None,
                retry_policy_override: None,
                metadata: HashMap::new(),
            })
        );
    }

    #[test]
    fn openai_compatible_target_keeps_pool_when_all_endpoints_match() {
        let target = build_openai_compatible_target(&[endpoint()], "pool-1", None).expect("target");

        assert_eq!(
            target,
            ExecutionTarget::Pool {
                pool_id: "pool-1".to_string(),
            }
        );
    }

    #[test]
    fn openai_compatible_target_rejects_target_without_match() {
        let error = build_openai_compatible_target(&[endpoint()], "pool-1", Some("anthropic"))
            .expect_err("target mismatch");

        assert_eq!(error.to_string(), "no provider matches target 'anthropic'");
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

    #[test]
    fn openai_completed_body_normalizes_anthropic_provider_output() {
        let body = openai_completed_chat_body(CompletedResponse {
            response: ChatResponseFinal {
                model: Some("claude-3-5-sonnet".to_string()),
                output_text: Some("pong".to_string()),
                raw: serde_json::json!({
                    "id": "msg_123",
                    "type": "message",
                }),
            },
            report: RequestReport {
                request_id: "req_456".to_string(),
                pool_id: Some("svc".to_string()),
                selected_endpoint_id: "anthropic-main".to_string(),
                selected_provider: ProviderKind::Anthropic,
                attempts: Vec::new(),
                usage: Some(unigateway_core::TokenUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(4),
                    total_tokens: Some(14),
                }),
                latency_ms: 10,
                started_at: std::time::SystemTime::UNIX_EPOCH,
                finished_at: std::time::SystemTime::UNIX_EPOCH,
                metadata: HashMap::new(),
            },
        });

        assert_eq!(
            body.get("object").and_then(serde_json::Value::as_str),
            Some("chat.completion")
        );
        assert_eq!(
            body.get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str),
            Some("pong")
        );
        assert_eq!(
            body.get("usage")
                .and_then(|usage| usage.get("completion_tokens"))
                .and_then(serde_json::Value::as_u64),
            Some(4)
        );
    }

    #[test]
    fn openai_stream_adapter_translates_anthropic_events() {
        let mut adapter = OpenAiChatStreamAdapter::default();

        let role_chunk = openai_sse_chunks_from_chat_chunk(
            "req_1",
            &mut adapter,
            ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "type": "message_start",
                    "model": "claude-3-5-sonnet",
                }),
            },
        );
        let content_chunk = openai_sse_chunks_from_chat_chunk(
            "req_1",
            &mut adapter,
            ChatResponseChunk {
                delta: Some("hello".to_string()),
                raw: serde_json::json!({
                    "type": "content_block_delta",
                    "delta": { "text": "hello" },
                }),
            },
        );
        let stop_chunk = openai_sse_chunks_from_chat_chunk(
            "req_1",
            &mut adapter,
            ChatResponseChunk {
                delta: None,
                raw: serde_json::json!({
                    "type": "message_stop",
                }),
            },
        );

        let role_payload = role_chunk[0]
            .strip_prefix(b"data: ")
            .and_then(|bytes| bytes.strip_suffix(b"\n\n"))
            .expect("role payload");
        let content_payload = content_chunk[0]
            .strip_prefix(b"data: ")
            .and_then(|bytes| bytes.strip_suffix(b"\n\n"))
            .expect("content payload");
        let stop_payload = stop_chunk[0]
            .strip_prefix(b"data: ")
            .and_then(|bytes| bytes.strip_suffix(b"\n\n"))
            .expect("stop payload");

        let role_json: serde_json::Value = serde_json::from_slice(role_payload).expect("role json");
        let content_json: serde_json::Value =
            serde_json::from_slice(content_payload).expect("content json");
        let stop_json: serde_json::Value = serde_json::from_slice(stop_payload).expect("stop json");

        assert_eq!(
            role_json
                .get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("role"))
                .and_then(serde_json::Value::as_str),
            Some("assistant")
        );
        assert_eq!(
            content_json
                .get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(serde_json::Value::as_str),
            Some("hello")
        );
        assert_eq!(
            stop_json
                .get("choices")
                .and_then(serde_json::Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(serde_json::Value::as_str),
            Some("stop")
        );
    }
}
