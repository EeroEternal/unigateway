use anyhow::{Result, anyhow};
use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use std::io;
use unigateway_core::{
    ExecutionTarget, GatewayError, ProxyResponsesRequest, ProxySession, ResponsesEvent,
    ResponsesFinal, TokenUsage,
};

use crate::host::RuntimeContext;

use super::targeting::{build_env_openai_pool, build_openai_compatible_target, prepare_core_pool};

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

async fn execute_openai_responses_via_core(
    runtime: &RuntimeContext<'_>,
    pool: unigateway_core::ProviderPool,
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

fn responses_usage_payload(usage: Option<&TokenUsage>) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.and_then(|usage| usage.input_tokens).unwrap_or(0),
        "output_tokens": usage.and_then(|usage| usage.output_tokens).unwrap_or(0),
        "total_tokens": usage.and_then(|usage| usage.total_tokens).unwrap_or(0),
    })
}

pub(super) fn without_response_tools(request: ProxyResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        tools: None,
        tool_choice: None,
        ..request
    }
}

fn should_retry_responses_without_tools(request: &ProxyResponsesRequest) -> bool {
    request.tools.is_some() || request.tool_choice.is_some()
}

pub(super) fn should_preserve_stream_error(
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
