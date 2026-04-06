mod chat;
mod core_adapter;
mod streaming;

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Json, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use tracing::{debug, info};

use crate::middleware::{
    GatewayAuth, error_json, extract_openai_api_key, extract_x_api_key, record_stat,
};
use crate::protocol::{
    UpstreamProtocol, anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, embed_response_to_openai_json, invoke_embeddings,
    invoke_responses_stream_with_connector, invoke_responses_with_connector,
    openai_payload_to_chat_request, openai_payload_to_embed_request,
    openai_payload_to_responses_request,
};
use crate::routing::{resolve_providers, target_provider_hint};
use crate::types::AppState;

use self::chat::{invoke_direct_chat, invoke_provider_chat};
use self::core_adapter::{
    try_anthropic_chat_via_core, try_openai_chat_via_core, try_openai_embeddings_via_core,
    try_openai_responses_via_core,
};

fn response_text(resp: &llm_connector::types::ResponsesResponse) -> String {
    if !resp.output_text.is_empty() {
        return resp.output_text.clone();
    }

    resp.output
        .as_ref()
        .map(|items| {
            items
                .iter()
                .flat_map(|item| item.content.as_ref().into_iter().flatten())
                .filter_map(|content| content.text.clone())
                .collect::<Vec<String>>()
                .join("")
        })
        .unwrap_or_default()
}

fn build_responses_stream_response_from_full(
    resp: llm_connector::types::ResponsesResponse,
) -> anyhow::Result<Response> {
    let response_id = resp.id.clone();
    let model = resp.model.clone();
    let text = response_text(&resp);
    let usage = resp.usage.clone();

    let mut chunks: Vec<Result<Bytes, std::io::Error>> = Vec::new();

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
            "response_id": response_id,
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
            "id": response_id,
            "object": "response",
            "model": model,
            "status": "completed",
            "usage": usage,
        }
    });
    chunks.push(Ok(Bytes::from(format!(
        "event: response.completed\ndata: {}\n\n",
        completed
    ))));
    chunks.push(Ok(Bytes::from("data: [DONE]\n\n")));

    let sse_stream = futures_util::stream::iter(chunks);
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(sse_stream))
        .map_err(|e| anyhow::anyhow!("build responses stream fallback: {e}"))
}

async fn invoke_responses_stream_with_fallback(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &llm_connector::types::ResponsesRequest,
    provider_family: Option<&str>,
) -> anyhow::Result<Response> {
    match invoke_responses_stream_with_connector(
        protocol,
        base_url,
        api_key,
        request,
        provider_family,
    )
    .await
    {
        Ok(stream) => {
            let sse_stream = stream.map(|event| match event {
                Ok(event) => {
                    // Ensure SSE data JSON carries `type` for strict Responses clients.
                    let mut event_data = event.data;
                    event_data
                        .entry("type".to_string())
                        .or_insert_with(|| serde_json::Value::String(event.event_type.clone()));
                    let data =
                        serde_json::to_string(&event_data).unwrap_or_else(|_| String::from("{}"));
                    let chunk = format!("event: {}\ndata: {}\n\n", event.event_type, data);
                    Ok::<Bytes, std::io::Error>(Bytes::from(chunk))
                }
                Err(err) => Err(std::io::Error::other(err.to_string())),
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(sse_stream))
                .map_err(|e| anyhow::anyhow!("build responses stream: {e}"))
        }
        Err(stream_err) => {
            tracing::warn!(error = %stream_err, "responses streaming failed, fallback to non-stream -> sse");
            let full_resp = invoke_responses_with_connector(
                protocol,
                base_url,
                api_key,
                request,
                provider_family,
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!("stream failed: {stream_err}; non-stream fallback failed: {e}")
            })?;

            build_responses_stream_response_from_full(full_resp)
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAI Chat
// ---------------------------------------------------------------------------

pub(crate) async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();
    let token = extract_openai_api_key(&headers, &state.config.openai_api_key);

    let mut request = match openai_payload_to_chat_request(&payload, &state.config.openai_model) {
        Ok(req) => req,
        Err(err) => return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}")),
    };

    let hint = target_provider_hint(&headers, &payload);
    let endpoint = "/v1/chat/completions";

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    if let Some(ref auth) = auth {
        match try_openai_chat_via_core(&state, &auth.key.service_id, hint.as_deref(), &request)
            .await
        {
            Ok(Some(response)) => {
                auth.finalize(&state).await;
                record_stat(&state, endpoint, 200, &start).await;
                return response;
            }
            Ok(None) => {}
            Err(error) => {
                auth.release(&state).await;
                let status = if error.to_string().contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::BAD_GATEWAY
                };
                record_stat(&state, endpoint, 500, &start).await;
                return error_json(status, &format!("core execution error: {error:#}"));
            }
        }

        let providers = match resolve_providers(
            &state.gateway,
            &auth.key.service_id,
            "openai",
            hint.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(msg) => {
                auth.release(&state).await;
                let status = if msg.contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                };
                return error_json(status, &msg);
            }
        };

        let original_model = request.model.clone();
        let mut last_err = String::from("unknown");

        for provider in &providers {
            request.model = provider.map_model(&original_model);

            let upstream_protocol = match provider.provider_type.as_str() {
                "anthropic" => UpstreamProtocol::Anthropic,
                _ => UpstreamProtocol::OpenAi,
            };

            debug!(
                provider_name = provider.name.as_str(),
                base_url = provider.base_url.as_str(),
                model = request.model.as_str(),
                upstream_protocol = ?upstream_protocol,
                "routing openai request to provider"
            );

            match invoke_provider_chat(
                upstream_protocol,
                provider,
                &request,
                chat_response_to_openai_json,
            )
            .await
            {
                Ok(resp) => {
                    auth.finalize(&state).await;
                    record_stat(&state, endpoint, 200, &start).await;
                    return resp;
                }
                Err(err) => {
                    tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                    last_err = format!("{err:#}");
                    continue;
                }
            }
        }

        auth.release(&state).await;
        record_stat(&state, endpoint, 500, &start).await;
        return error_json(
            StatusCode::BAD_GATEWAY,
            &format!("all providers failed, last: {last_err}"),
        );
    }

    // No gateway key — use env config
    let api_key = fallback_api_key(&token, &state.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    let base_url = &state.config.openai_base_url;
    match invoke_direct_chat(
        UpstreamProtocol::OpenAi,
        base_url,
        &api_key,
        &request,
        None,
        chat_response_to_openai_json,
    )
    .await
    {
        Ok(resp) => {
            record_stat(&state, endpoint, 200, &start).await;
            resp
        }
        Err(err) => {
            record_stat(&state, endpoint, 500, &start).await;
            error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
        }
    }
}

// ---------------------------------------------------------------------------
// OpenAI Responses
// ---------------------------------------------------------------------------

pub(crate) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();
    let token = extract_openai_api_key(&headers, &state.config.openai_api_key);

    let mut request =
        match openai_payload_to_responses_request(&payload, &state.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };
    let stream = request.stream.unwrap_or(false);

    let hint = target_provider_hint(&headers, &payload);
    let endpoint = "/v1/responses";
    let original_model = request.model.clone();

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    if let Some(ref auth) = auth {
        match try_openai_responses_via_core(
            &state,
            &auth.key.service_id,
            hint.as_deref(),
            &request,
            &payload,
        )
        .await
        {
            Ok(Some(response)) => {
                auth.finalize(&state).await;
                record_stat(&state, endpoint, 200, &start).await;
                return response;
            }
            Ok(None) => {}
            Err(error) => {
                auth.release(&state).await;
                let status = if error.to_string().contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::BAD_GATEWAY
                };
                record_stat(&state, endpoint, 500, &start).await;
                return error_json(status, &format!("core execution error: {error:#}"));
            }
        }

        let providers = match resolve_providers(
            &state.gateway,
            &auth.key.service_id,
            "openai",
            hint.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(msg) => {
                auth.release(&state).await;
                let status = if msg.contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                };
                return error_json(status, &msg);
            }
        };

        let mut last_err = String::from("unknown");
        for provider in &providers {
            request.model = provider.map_model(&original_model);
            let req_primary = request.clone();

            let upstream_protocol = match provider.provider_type.as_str() {
                "anthropic" => UpstreamProtocol::Anthropic,
                _ => UpstreamProtocol::OpenAi,
            };

            let mut result = if stream {
                invoke_responses_stream_with_fallback(
                    upstream_protocol,
                    &provider.base_url,
                    &provider.api_key,
                    &req_primary,
                    provider.family_id.as_deref(),
                )
                .await
            } else {
                invoke_responses_with_connector(
                    upstream_protocol,
                    &provider.base_url,
                    &provider.api_key,
                    &req_primary,
                    provider.family_id.as_deref(),
                )
                .await
                .map(|resp| (StatusCode::OK, Json(resp)).into_response())
            };

            // Compatibility retry: when the request contains tools and the
            // upstream fails (e.g. tool schemas that cannot be mapped to chat
            // fallback types, or providers that don't support them), retry
            // without tools so the model can still produce a text response.
            if let Err(_err) = &result {
                let has_tools = req_primary
                    .tools
                    .as_ref()
                    .is_some_and(|t| t.as_array().is_some_and(|a| !a.is_empty()));

                if has_tools {
                    let mut req_compat = req_primary.clone();
                    req_compat.tools = None;
                    req_compat.tool_choice = None;

                    result = if stream {
                        invoke_responses_stream_with_fallback(
                            upstream_protocol,
                            &provider.base_url,
                            &provider.api_key,
                            &req_compat,
                            provider.family_id.as_deref(),
                        )
                        .await
                    } else {
                        invoke_responses_with_connector(
                            upstream_protocol,
                            &provider.base_url,
                            &provider.api_key,
                            &req_compat,
                            provider.family_id.as_deref(),
                        )
                        .await
                        .map(|resp| (StatusCode::OK, Json(resp)).into_response())
                    };
                }
            }

            match result {
                Ok(resp) => {
                    auth.finalize(&state).await;
                    record_stat(&state, endpoint, 200, &start).await;
                    return resp;
                }
                Err(err) => {
                    tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream responses error, trying next");
                    last_err = format!("{err:#}");
                }
            }
        }

        auth.release(&state).await;
        record_stat(&state, endpoint, 500, &start).await;
        return error_json(
            StatusCode::BAD_GATEWAY,
            &format!("all providers failed, last: {last_err}"),
        );
    }

    let api_key = fallback_api_key(&token, &state.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    request.model = original_model;
    let req_primary = request.clone();

    let mut result = if stream {
        invoke_responses_stream_with_fallback(
            UpstreamProtocol::OpenAi,
            &state.config.openai_base_url,
            &api_key,
            &req_primary,
            None,
        )
        .await
    } else {
        invoke_responses_with_connector(
            UpstreamProtocol::OpenAi,
            &state.config.openai_base_url,
            &api_key,
            &req_primary,
            None,
        )
        .await
        .map(|resp| (StatusCode::OK, Json(resp)).into_response())
    };

    if let Err(err) = &result {
        let err_msg = format!("{err:#}");
        let should_retry_without_tools = err_msg.contains("Failed to map responses.tools")
            || err_msg.contains("Failed to map responses.tool_choice");

        if should_retry_without_tools {
            let mut req_compat = req_primary.clone();
            req_compat.tools = None;
            req_compat.tool_choice = None;

            result = if stream {
                invoke_responses_stream_with_fallback(
                    UpstreamProtocol::OpenAi,
                    &state.config.openai_base_url,
                    &api_key,
                    &req_compat,
                    None,
                )
                .await
            } else {
                invoke_responses_with_connector(
                    UpstreamProtocol::OpenAi,
                    &state.config.openai_base_url,
                    &api_key,
                    &req_compat,
                    None,
                )
                .await
                .map(|resp| (StatusCode::OK, Json(resp)).into_response())
            };
        }
    }

    match result {
        Ok(resp) => {
            record_stat(&state, endpoint, 200, &start).await;
            resp
        }
        Err(err) => {
            record_stat(&state, endpoint, 500, &start).await;
            error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Anthropic Messages
// ---------------------------------------------------------------------------

pub(crate) async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();
    let has_x_api_key = headers.get("x-api-key").is_some();
    let has_bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("Bearer "))
        .unwrap_or(false);
    let token = extract_x_api_key(&headers, &state.config.anthropic_api_key);
    info!(
        endpoint = "/v1/messages",
        has_x_api_key,
        has_bearer,
        token_present = !token.is_empty(),
        model = payload.get("model").and_then(|v| v.as_str()).unwrap_or(""),
        "received anthropic request"
    );

    let mut request =
        match anthropic_payload_to_chat_request(&payload, &state.config.anthropic_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    let endpoint = "/v1/messages";
    let hint = target_provider_hint(&headers, &payload);

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    info!(
        endpoint,
        gateway_key_matched = auth.is_some(),
        token_present = !token.is_empty(),
        "anthropic request authentication result"
    );

    if let Some(ref auth) = auth {
        match try_anthropic_chat_via_core(&state, &auth.key.service_id, hint.as_deref(), &request)
            .await
        {
            Ok(Some(response)) => {
                auth.finalize(&state).await;
                record_stat(&state, endpoint, 200, &start).await;
                return response;
            }
            Ok(None) => {}
            Err(error) => {
                auth.release(&state).await;
                let status = if error.to_string().contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::BAD_GATEWAY
                };
                record_stat(&state, endpoint, 500, &start).await;
                return error_json(status, &format!("core execution error: {error:#}"));
            }
        }

        let providers = match resolve_providers(
            &state.gateway,
            &auth.key.service_id,
            "anthropic",
            hint.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(msg) => {
                auth.release(&state).await;
                return error_json(StatusCode::SERVICE_UNAVAILABLE, &msg);
            }
        };

        let original_model = request.model.clone();
        let mut last_err = String::from("unknown");

        for provider in &providers {
            request.model = provider.map_model(&original_model);

            // If we are routing Anthropic request to an OpenAI-compatible provider (like Moonshot/Kimi),
            // we MUST set UpstreamProtocol::OpenAi.
            // resolve_providers returns providers with their configured type.
            // If the provider is configured as "openai", we use OpenAi protocol.
            let upstream_protocol = match provider.provider_type.as_str() {
                "anthropic" => UpstreamProtocol::Anthropic,
                _ => UpstreamProtocol::OpenAi,
            };

            info!(
                provider_name = provider.name.as_str(),
                base_url = provider.base_url.as_str(),
                model = request.model.as_str(),
                upstream_protocol = ?upstream_protocol,
                "routing anthropic request to provider"
            );

            // SPECIAL HANDLING FOR AUTHENTICATION
            // If we are routing to an OpenAI provider, we need to ensure the request has the correct API key header.
            // invoke_provider_chat uses provider.api_key.
            // But we must ensure that llm-connector handles the protocol conversion correctly,
            // specifically putting the key in "Authorization: Bearer" for OpenAI,
            // and "x-api-key" for Anthropic.

            match invoke_provider_chat(
                upstream_protocol,
                provider,
                &request,
                chat_response_to_anthropic_json,
            )
            .await
            {
                Ok(resp) => {
                    auth.finalize(&state).await;
                    record_stat(&state, endpoint, 200, &start).await;
                    return resp;
                }
                Err(err) => {
                    tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                    last_err = format!("{err:#}");
                    continue;
                }
            }
        }

        auth.release(&state).await;
        record_stat(&state, endpoint, 500, &start).await;
        return error_json(
            StatusCode::BAD_GATEWAY,
            &format!("all providers failed, last: {last_err}"),
        );
    }

    // No gateway key — use env config
    let api_key = fallback_api_key(&token, &state.config.anthropic_api_key);
    info!(
        endpoint,
        token_present = !token.is_empty(),
        env_key_present = !state.config.anthropic_api_key.is_empty(),
        using_env_fallback = token.is_empty() && !state.config.anthropic_api_key.is_empty(),
        "anthropic request falling back to env upstream key"
    );
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    match invoke_direct_chat(
        UpstreamProtocol::Anthropic,
        &state.config.anthropic_base_url,
        &api_key,
        &request,
        None,
        chat_response_to_anthropic_json,
    )
    .await
    {
        Ok(resp) => {
            record_stat(&state, endpoint, 200, &start).await;
            resp
        }
        Err(err) => {
            record_stat(&state, endpoint, 500, &start).await;
            error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

pub(crate) async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();
    let token = extract_openai_api_key(&headers, &state.config.openai_api_key);

    let mut embed_request =
        match openai_payload_to_embed_request(&payload, &state.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    let endpoint = "/v1/embeddings";
    let hint = target_provider_hint(&headers, &payload);

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    if let Some(ref auth) = auth {
        match try_openai_embeddings_via_core(
            &state,
            &auth.key.service_id,
            hint.as_deref(),
            &embed_request,
            &payload,
        )
        .await
        {
            Ok(Some(response)) => {
                auth.finalize(&state).await;
                record_stat(&state, endpoint, 200, &start).await;
                return response;
            }
            Ok(None) => {}
            Err(error) => {
                auth.release(&state).await;
                let status = if error.to_string().contains("matches target") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::BAD_GATEWAY
                };
                record_stat(&state, endpoint, 500, &start).await;
                return error_json(status, &format!("core execution error: {error:#}"));
            }
        }

        let providers = match resolve_providers(
            &state.gateway,
            &auth.key.service_id,
            "openai",
            hint.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(msg) => {
                auth.release(&state).await;
                return error_json(StatusCode::SERVICE_UNAVAILABLE, &msg);
            }
        };

        let original_model = embed_request.model.clone();
        let mut last_err = String::from("unknown");

        for provider in &providers {
            embed_request.model = provider.map_model(&original_model);

            debug!(
                provider_name = provider.name.as_str(),
                model = embed_request.model.as_str(),
                "routing embeddings request to provider"
            );

            match invoke_embeddings(&provider.base_url, &provider.api_key, &embed_request).await {
                Ok(resp) => {
                    auth.finalize(&state).await;
                    record_stat(&state, endpoint, 200, &start).await;
                    return (StatusCode::OK, Json(embed_response_to_openai_json(&resp)))
                        .into_response();
                }
                Err(err) => {
                    tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                    last_err = format!("{err:#}");
                    continue;
                }
            }
        }

        auth.release(&state).await;
        record_stat(&state, endpoint, 500, &start).await;
        return error_json(
            StatusCode::BAD_GATEWAY,
            &format!("all providers failed, last: {last_err}"),
        );
    }

    // No gateway key — use env config
    let api_key = fallback_api_key(&token, &state.config.openai_api_key);
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    match invoke_embeddings(&state.config.openai_base_url, &api_key, &embed_request).await {
        Ok(resp) => {
            record_stat(&state, endpoint, 200, &start).await;
            (StatusCode::OK, Json(embed_response_to_openai_json(&resp))).into_response()
        }
        Err(err) => {
            record_stat(&state, endpoint, 500, &start).await;
            error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fallback_api_key(token: &str, env_key: &str) -> String {
    if !token.is_empty() {
        token.to_string()
    } else {
        env_key.to_string()
    }
}
