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
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProviderKind, ProviderPool,
    ProxyChatRequest, ProxySession,
};

use crate::host::RuntimeContext;

use super::targeting::{
    build_env_anthropic_pool, build_env_openai_pool, build_execution_target, prepare_core_pool,
};

mod streaming;

use streaming::drive_anthropic_chat_stream;
pub(crate) use streaming::{OpenAiChatStreamAdapter, openai_sse_chunks_from_chat_chunk};

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

                let chunks: Vec<Result<Bytes, std::io::Error>> = match item {
                    Ok(chunk) => {
                        let mut adapter = adapter_state.lock().expect("adapter lock");
                        openai_sse_chunks_from_chat_chunk(&request_id, &mut adapter, chunk)
                            .into_iter()
                            .map(Ok)
                            .collect()
                    }
                    Err(error) => vec![Err(std::io::Error::other(error.to_string()))],
                };

                futures_util::stream::iter(chunks)
            });
            let done = futures_util::stream::once(async {
                Ok::<Bytes, std::io::Error>(Bytes::from("data: [DONE]\n\n"))
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

pub(super) fn anthropic_completed_chat_body(
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
        "usage": streaming::anthropic_usage_payload(result.report.usage.as_ref()),
    })
}

pub(super) fn openai_completed_chat_body(
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
