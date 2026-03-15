use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Json, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{StreamChunk, StreamFormat};
use tracing::debug;

use crate::middleware::{
    GatewayAuth, error_json, extract_bearer_token, extract_x_api_key, record_stat,
};
use crate::protocol::{
    UpstreamProtocol, anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, embed_response_to_openai_json, invoke_embeddings,
    invoke_with_connector, invoke_with_connector_stream, openai_payload_to_chat_request,
    openai_payload_to_embed_request,
};
use crate::routing::{ResolvedProvider, resolve_providers, target_provider_hint};
use crate::types::AppState;

// ---------------------------------------------------------------------------
// OpenAI Chat
// ---------------------------------------------------------------------------

pub(crate) async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();
    let token = extract_bearer_token(&headers, &state.config.openai_api_key);

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
        let is_stream = request.stream == Some(true);

        for provider in &providers {
            request.model = provider.map_model(&original_model);

            debug!(
                provider_name = provider.name.as_str(),
                base_url = provider.base_url.as_str(),
                model = request.model.as_str(),
                "routing openai request to provider"
            );

            if is_stream {
                match try_chat_stream(UpstreamProtocol::OpenAi, provider, &request).await {
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
            } else {
                match invoke_with_connector(
                    UpstreamProtocol::OpenAi,
                    &provider.base_url,
                    &provider.api_key,
                    &request,
                    provider.family_id.as_deref(),
                )
                .await
                {
                    Ok(resp) => {
                        auth.finalize(&state).await;
                        record_stat(&state, endpoint, 200, &start).await;
                        return (StatusCode::OK, Json(chat_response_to_openai_json(&resp)))
                            .into_response();
                    }
                    Err(err) => {
                        tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                        last_err = format!("{err:#}");
                        continue;
                    }
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
    if request.stream == Some(true) {
        match try_chat_stream_raw(UpstreamProtocol::OpenAi, base_url, &api_key, &request, None)
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
    } else {
        match invoke_with_connector(UpstreamProtocol::OpenAi, base_url, &api_key, &request, None)
            .await
        {
            Ok(resp) => {
                record_stat(&state, endpoint, 200, &start).await;
                (StatusCode::OK, Json(chat_response_to_openai_json(&resp))).into_response()
            }
            Err(err) => {
                record_stat(&state, endpoint, 500, &start).await;
                error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
            }
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
    let token = extract_x_api_key(&headers, &state.config.anthropic_api_key);

    let mut request =
        match anthropic_payload_to_chat_request(&payload, &state.config.anthropic_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    let endpoint = "/v1/messages";

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    if let Some(ref auth) = auth {
        let providers = match resolve_providers(
            &state.gateway,
            &auth.key.service_id,
            "anthropic",
            None,
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
        let is_stream = request.stream == Some(true);

        for provider in &providers {
            request.model = provider.map_model(&original_model);

            debug!(
                provider_name = provider.name.as_str(),
                base_url = provider.base_url.as_str(),
                model = request.model.as_str(),
                "routing anthropic request to provider"
            );

            if is_stream {
                match try_chat_stream(UpstreamProtocol::Anthropic, provider, &request).await {
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
            } else {
                match invoke_with_connector(
                    UpstreamProtocol::Anthropic,
                    &provider.base_url,
                    &provider.api_key,
                    &request,
                    None,
                )
                .await
                {
                    Ok(resp) => {
                        auth.finalize(&state).await;
                        record_stat(&state, endpoint, 200, &start).await;
                        return (StatusCode::OK, Json(chat_response_to_anthropic_json(&resp)))
                            .into_response();
                    }
                    Err(err) => {
                        tracing::warn!(provider = provider.name.as_str(), error = %err, "upstream error, trying next");
                        last_err = format!("{err:#}");
                        continue;
                    }
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
    if api_key.is_empty() {
        return error_json(StatusCode::BAD_REQUEST, "missing upstream api key");
    }

    if request.stream == Some(true) {
        match try_chat_stream_raw(
            UpstreamProtocol::Anthropic,
            &state.config.anthropic_base_url,
            &api_key,
            &request,
            None,
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
    } else {
        match invoke_with_connector(
            UpstreamProtocol::Anthropic,
            &state.config.anthropic_base_url,
            &api_key,
            &request,
            None,
        )
        .await
        {
            Ok(resp) => {
                record_stat(&state, endpoint, 200, &start).await;
                (StatusCode::OK, Json(chat_response_to_anthropic_json(&resp))).into_response()
            }
            Err(err) => {
                record_stat(&state, endpoint, 500, &start).await;
                error_json(StatusCode::BAD_GATEWAY, &format!("upstream error: {err:#}"))
            }
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
    let token = extract_bearer_token(&headers, &state.config.openai_api_key);

    let mut embed_request =
        match openai_payload_to_embed_request(&payload, &state.config.openai_model) {
            Ok(req) => req,
            Err(err) => {
                return error_json(StatusCode::BAD_REQUEST, &format!("invalid request: {err}"));
            }
        };

    let endpoint = "/v1/embeddings";

    let auth = match GatewayAuth::try_authenticate(&state, &token).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    if let Some(ref auth) = auth {
        let providers =
            match resolve_providers(&state.gateway, &auth.key.service_id, "openai", None).await {
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

async fn try_chat_stream(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &llm_connector::types::ChatRequest,
) -> Result<Response, anyhow::Error> {
    try_chat_stream_raw(
        protocol,
        &provider.base_url,
        &provider.api_key,
        request,
        provider.family_id.as_deref(),
    )
    .await
}

async fn try_chat_stream_raw(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &llm_connector::types::ChatRequest,
    family_id: Option<&str>,
) -> Result<Response, anyhow::Error> {
    let stream =
        invoke_with_connector_stream(protocol, base_url, api_key, request, family_id).await?;
    type BoxErr = Box<dyn std::error::Error + Send + Sync>;
    let sse_stream = stream.map(|r: Result<_, llm_connector::error::LlmConnectorError>| {
        r.map_err(|e| -> BoxErr { Box::new(std::io::Error::other(e.to_string())) })
            .and_then(|resp| {
                StreamChunk::from_openai(&resp, StreamFormat::SSE)
                    .map(|c| Bytes::from(c.to_sse()))
                    .map_err(|e: serde_json::Error| -> BoxErr { Box::new(e) })
            })
    });
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(sse_stream),
    )
        .into_response())
}
