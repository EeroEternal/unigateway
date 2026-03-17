mod chat;
mod streaming;

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tracing::{debug, info};

use crate::middleware::{
    GatewayAuth, error_json, extract_bearer_token, extract_x_api_key, record_stat,
};
use crate::protocol::{
    UpstreamProtocol, anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, embed_response_to_openai_json, invoke_embeddings,
    openai_payload_to_chat_request, openai_payload_to_embed_request,
};
use crate::routing::{resolve_providers, target_provider_hint};
use crate::types::AppState;

use self::chat::{invoke_direct_chat, invoke_provider_chat};

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
