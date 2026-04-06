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

    if pool
        .endpoints
        .iter()
        .any(|endpoint| endpoint.provider_kind != ProviderKind::OpenAiCompatible)
    {
        return Ok(None);
    }

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
    let pool = match prepare_openai_compatible_pool(runtime, service_id).await? {
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
    let pool = match prepare_openai_compatible_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
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

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
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
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = match runtime
        .core_engine()
        .proxy_responses(request.clone(), target.clone())
        .await
    {
        Ok(session) => session,
        Err(error) if should_fallback_to_legacy_responses(&error) => return Ok(None),
        Err(error) if should_retry_responses_without_tools(&request) => {
            let retry_request = without_response_tools(request);
            match runtime
                .core_engine()
                .proxy_responses(retry_request, target)
                .await
            {
                Ok(session) => session,
                Err(error) if should_fallback_to_legacy_responses(&error) => return Ok(None),
                Err(error) => return Err(anyhow!(error.to_string())),
            }
        }
        Err(error) => return Err(anyhow!(error.to_string())),
    };

    Ok(Some(responses_session_to_openai_response(session)))
}

async fn prepare_openai_compatible_pool(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
) -> Result<Option<ProviderPool>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
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

fn should_fallback_to_legacy_responses(error: &GatewayError) -> bool {
    matches!(
        error,
        GatewayError::NotImplemented(_)
            | GatewayError::UpstreamHttp { status: 404, .. }
            | GatewayError::UpstreamHttp { status: 405, .. }
    )
}

pub fn responses_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.is_object()
}

pub fn embeddings_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| matches!(key.as_str(), "model" | "input"))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{
        ChatResponseFinal, CompletedResponse, Endpoint, GatewayError, ModelPolicy, ProviderKind,
        ProxyResponsesRequest, RequestReport, SecretString,
    };

    use super::{
        anthropic_completed_chat_body, build_env_anthropic_pool, build_env_openai_pool,
        embeddings_payload_is_core_compatible, endpoint_matches_hint,
        responses_payload_is_core_compatible, should_fallback_to_legacy_responses,
        without_response_tools,
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
    fn responses_core_bridge_accepts_supported_safe_subset() {
        assert!(responses_payload_is_core_compatible(&serde_json::json!({
            "model": "gpt-4.1-mini",
            "input": "hello",
            "stream": true,
            "instructions": "be terse",
            "temperature": 0.2,
            "top_p": 0.9,
            "max_output_tokens": 128,
            "tools": [],
            "tool_choice": "auto",
            "previous_response_id": "resp_prev",
            "metadata": {"trace_id": "abc"},
            "reasoning": {"effort": "high"},
            "target_provider": "deepseek",
        })));
        assert!(!responses_payload_is_core_compatible(&serde_json::json!(
            "hello"
        )));
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
    fn responses_legacy_fallback_detects_missing_endpoint() {
        assert!(should_fallback_to_legacy_responses(
            &GatewayError::UpstreamHttp {
                status: 404,
                body: Some("not found".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
        assert!(!should_fallback_to_legacy_responses(
            &GatewayError::UpstreamHttp {
                status: 500,
                body: Some("boom".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
    }

    #[test]
    fn embeddings_core_bridge_only_accepts_minimal_subset() {
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
