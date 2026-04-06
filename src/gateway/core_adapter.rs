use std::io;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{
    ChatRequest, EmbedRequest, Message as ConnectorMessage, ResponsesRequest, Role,
};
use tokio::sync::mpsc;
use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, Endpoint,
    EndpointRef, ExecutionPlan, ExecutionTarget, Message, MessageRole, ProviderKind,
    ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest, ProxySession, ResponsesEvent,
    ResponsesFinal, StreamingResponse, TokenUsage,
};

use crate::config::core_sync::build_core_pool_for_service;
use crate::types::AppState;

pub(super) async fn try_anthropic_chat_via_core(
    state: &Arc<AppState>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(state, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    state
        .core_engine
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let session = state
        .core_engine
        .proxy_chat(to_core_chat_request(request), target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_anthropic_response(
        session,
        request.model.clone(),
    )))
}

pub(super) async fn try_openai_chat_via_core(
    state: &Arc<AppState>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Option<Response>> {
    let pool = match build_core_pool_for_service(&state.gateway, service_id).await {
        Ok(pool) => pool,
        Err(error)
            if error
                .to_string()
                .contains("unsupported core routing strategy") =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    if pool
        .endpoints
        .iter()
        .any(|endpoint| endpoint.provider_kind != ProviderKind::OpenAiCompatible)
    {
        return Ok(None);
    }

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    state
        .core_engine
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let core_request = to_core_chat_request(request);
    let session = state
        .core_engine
        .proxy_chat(core_request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_openai_response(session)))
}

pub(super) async fn try_openai_responses_via_core(
    state: &Arc<AppState>,
    service_id: &str,
    hint: Option<&str>,
    request: &ResponsesRequest,
    payload: &serde_json::Value,
) -> Result<Option<Response>> {
    if !responses_payload_is_core_compatible(payload)
        || request.tools.is_some()
        || request.tool_choice.is_some()
    {
        return Ok(None);
    }

    let pool = match prepare_openai_compatible_pool(state, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    state
        .core_engine
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let session = state
        .core_engine
        .proxy_responses(
            ProxyResponsesRequest {
                model: request.model.clone(),
                input: request.input.clone(),
                instructions: request.instructions.clone(),
                temperature: request.temperature,
                top_p: request.top_p,
                max_output_tokens: request.max_output_tokens,
                stream: request.stream.unwrap_or(false),
                previous_response_id: request.previous_response_id.clone(),
                request_metadata: request.metadata.clone(),
                metadata: std::collections::HashMap::new(),
            },
            target,
        )
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(responses_session_to_openai_response(session)))
}

pub(super) async fn try_openai_embeddings_via_core(
    state: &Arc<AppState>,
    service_id: &str,
    hint: Option<&str>,
    request: &EmbedRequest,
    payload: &serde_json::Value,
) -> Result<Option<Response>> {
    if !embeddings_payload_is_core_compatible(payload) {
        return Ok(None);
    }

    let pool = match prepare_openai_compatible_pool(state, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    state
        .core_engine
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let response = state
        .core_engine
        .proxy_embeddings(
            ProxyEmbeddingsRequest {
                model: request.model.clone(),
                input: request.input.clone(),
                metadata: std::collections::HashMap::new(),
            },
            target,
        )
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

async fn prepare_openai_compatible_pool(
    state: &Arc<AppState>,
    service_id: &str,
) -> Result<Option<unigateway_core::ProviderPool>> {
    let pool = match prepare_core_pool(state, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    if pool
        .endpoints
        .iter()
        .any(|endpoint| endpoint.provider_kind != ProviderKind::OpenAiCompatible)
    {
        return Ok(None);
    }

    Ok(Some(pool))
}

async fn prepare_core_pool(
    state: &Arc<AppState>,
    service_id: &str,
) -> Result<Option<unigateway_core::ProviderPool>> {
    match build_core_pool_for_service(&state.gateway, service_id).await {
        Ok(pool) => Ok(Some(pool)),
        Err(error)
            if error
                .to_string()
                .contains("unsupported core routing strategy") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
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

fn to_core_chat_request(request: &ChatRequest) -> ProxyChatRequest {
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

fn responses_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object.keys().all(|key| {
            matches!(
                key.as_str(),
                "model"
                    | "input"
                    | "stream"
                    | "instructions"
                    | "temperature"
                    | "top_p"
                    | "max_output_tokens"
                    | "previous_response_id"
                    | "metadata"
            )
        })
    })
}

fn embeddings_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| matches!(key.as_str(), "model" | "input"))
    })
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

fn chat_session_to_openai_response(
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

    use unigateway_core::{
        ChatResponseFinal, CompletedResponse, Endpoint, ModelPolicy, ProviderKind, RequestReport,
        SecretString,
    };

    use super::{
        anthropic_completed_chat_body, embeddings_payload_is_core_compatible,
        endpoint_matches_hint, responses_payload_is_core_compatible,
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
    fn responses_core_adapter_accepts_supported_safe_subset() {
        assert!(responses_payload_is_core_compatible(&serde_json::json!({
            "model": "gpt-4.1-mini",
            "input": "hello",
            "stream": true,
            "instructions": "be terse",
            "temperature": 0.2,
            "top_p": 0.9,
            "max_output_tokens": 128,
            "previous_response_id": "resp_prev",
            "metadata": {"trace_id": "abc"},
        })));
        assert!(!responses_payload_is_core_compatible(&serde_json::json!({
            "model": "gpt-4.1-mini",
            "input": "hello",
            "tools": [],
        })));
    }

    #[test]
    fn embeddings_core_adapter_only_accepts_minimal_subset() {
        assert!(embeddings_payload_is_core_compatible(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["hello"],
        })));
        assert!(!embeddings_payload_is_core_compatible(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["hello"],
            "encoding_format": "float",
        })));
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
