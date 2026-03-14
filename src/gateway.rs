use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{Json, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{StreamChunk, StreamFormat};
use llm_providers::get_endpoint;
use serde_json::json;
use tracing::debug;

use crate::config::{GatewayApiKey, RuntimeRateState};
use crate::protocol::{
    UpstreamProtocol, anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, invoke_with_connector, invoke_with_connector_stream,
    openai_payload_to_chat_request,
};
use crate::storage::map_model_name;
use crate::types::AppState;

/// Resolves upstream base_url and optional family_id. When endpoint_id is set, uses llm_providers
/// to get (base_url, family_id); otherwise (or when get_endpoint fails) uses provider_base_url.
fn resolve_upstream(
    provider_base_url: Option<String>,
    endpoint_id: Option<&str>,
) -> Option<(String, Option<String>)> {
    if let Some(eid) = endpoint_id {
        let eid = eid.trim();
        if !eid.is_empty() {
            if let Some((family_id, endpoint)) = get_endpoint(eid) {
                return Some((endpoint.base_url.to_string(), Some(family_id.to_string())));
            }
            tracing::debug!("get_endpoint({:?}) returned None, falling back to provider base_url", eid);
        }
    }

    let url = provider_base_url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    Some((url.to_string(), None))
}

fn target_provider_hint(headers: &HeaderMap, payload: &serde_json::Value) -> Option<String> {
    let from_header = headers
        .get("x-unigateway-provider")
        .or_else(|| headers.get("x-target-vendor"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    if from_header.is_some() {
        return from_header;
    }

    payload
        .get("target_vendor")
        .or_else(|| payload.get("target_provider"))
        .or_else(|| payload.get("provider"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

pub(crate) async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();

    let api_key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            if state.config.openai_api_key.is_empty() {
                None
            } else {
                Some(format!("Bearer {}", state.config.openai_api_key))
            }
        });

    let token = api_key
        .as_deref()
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    let mut request = match openai_payload_to_chat_request(&payload, &state.config.openai_model) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":format!("invalid request: {err}")}})),
            )
                .into_response();
        }
    };

    let mut upstream_base_url = state.config.openai_base_url.clone();
    let mut upstream_api_key = token.to_string();
    let mut upstream_family_id: Option<String> = None;
    let mut release_gateway_key: Option<String> = None;
    let target_provider = target_provider_hint(&headers, &payload);

    if !token.is_empty() {
        match state.gateway.find_gateway_api_key(token).await {
            Some(gateway_key) => {
                if gateway_key.is_active == 0 {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error":{"message":"api key is inactive"}})),
                    )
                        .into_response();
                }

                if let Some(quota_limit) = gateway_key.quota_limit
                    && gateway_key.used_quota >= quota_limit
                {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({"error":{"message":"api key quota exceeded"}})),
                    )
                        .into_response();
                }

                if let Err(resp) = acquire_runtime_limit(&state, &gateway_key).await {
                    return resp;
                }

                let provider = if let Some(ref hint) = target_provider {
                    match state
                        .gateway
                        .select_provider_for_service_with_hint(&gateway_key.service_id, "openai", hint)
                        .await
                    {
                        Some(p) => p,
                        None => {
                            release_runtime_inflight(&state, &gateway_key.key).await;
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error":{"message":format!("no provider matches target '{}'", hint)}})),
                            )
                                .into_response();
                        }
                    }
                } else {
                    let Some(provider) = state.gateway.select_provider_for_service(&gateway_key.service_id, "openai").await else {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":{"message":"no provider bound for service/openai"}})),
                        )
                            .into_response();
                    };
                    provider
                };

                let Some((base_url, family_id)) = resolve_upstream(
                    provider.base_url.clone(),
                    provider.endpoint_id.as_deref(),
                ) else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider base_url missing"}})),
                    )
                        .into_response();
                };
                let Some(provider_api_key) = provider.api_key.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider api_key missing"}})),
                    )
                        .into_response();
                };

                if let Some(mapped_model) =
                    map_model_name(provider.model_mapping.as_deref(), &request.model)
                {
                    request.model = mapped_model;
                }

                debug!(
                    service_id = gateway_key.service_id.as_str(),
                    provider_name = provider.name.as_str(),
                    provider_type = provider.provider_type.as_str(),
                    endpoint_id = provider.endpoint_id.as_deref().unwrap_or(""),
                    target_provider = target_provider.as_deref().unwrap_or(""),
                    upstream_base_url = base_url.as_str(),
                    upstream_family_id = family_id.as_deref().unwrap_or(""),
                    mapped_model = request.model.as_str(),
                    "selected upstream provider for openai request"
                );

                upstream_base_url = base_url;
                upstream_api_key = provider_api_key;
                upstream_family_id = family_id;
                release_gateway_key = Some(gateway_key.key.clone());

                state.gateway.increment_used_quota(&gateway_key.key).await;
            }
            None => {}
        }
    }

    if upstream_api_key.is_empty() {
        upstream_api_key = state.config.openai_api_key.clone();
    }
    if upstream_api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"missing upstream api key"}})),
        )
            .into_response();
    }

    if request.stream == Some(true) {
        match invoke_with_connector_stream(
            &upstream_base_url,
            &upstream_api_key,
            &request,
            upstream_family_id.as_deref(),
        )
        .await
        {
            Ok(stream) => {
                type BoxErr = Box<dyn std::error::Error + Send + Sync>;
                let sse_stream = stream.map(|r: Result<_, llm_connector::error::LlmConnectorError>| {
                    r.map_err(|e| -> BoxErr { Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) })
                        .and_then(|resp| {
                            StreamChunk::from_openai(&resp, StreamFormat::SSE)
                                .map(|c| Bytes::from(c.to_sse()))
                                .map_err(|e: serde_json::Error| -> BoxErr { Box::new(e) })
                        })
                });
                let body = Body::from_stream(sse_stream);
                if let Some(gateway_key) = release_gateway_key {
                    release_runtime_inflight(&state, &gateway_key).await;
                }
                state
                    .gateway
                    .record_stat("/v1/chat/completions", 200, start.elapsed().as_millis() as i64)
                    .await;
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    body,
                )
                    .into_response();
            }
            Err(err) => {
                if let Some(gateway_key) = release_gateway_key {
                    release_runtime_inflight(&state, &gateway_key).await;
                }
                state
                    .gateway
                    .record_stat("/v1/chat/completions", 500, start.elapsed().as_millis() as i64)
                    .await;
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error":{"message":format!("upstream error: {err:#}")}})),
                )
                    .into_response();
            }
        }
    }

    match invoke_with_connector(
        UpstreamProtocol::OpenAi,
        &upstream_base_url,
        &upstream_api_key,
        &request,
        upstream_family_id.as_deref(),
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_openai_json(&resp);
            let status = StatusCode::OK;
            state
                .gateway
                .record_stat("/v1/chat/completions", 200, start.elapsed().as_millis() as i64)
                .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            state
                .gateway
                .record_stat("/v1/chat/completions", 500, start.elapsed().as_millis() as i64)
                .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err:#}")}})),
            )
                .into_response()
        }
    }
}

pub(crate) async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let start = Instant::now();

    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            if state.config.anthropic_api_key.is_empty() {
                None
            } else {
                Some(state.config.anthropic_api_key.clone())
            }
        })
        .unwrap_or_default();

    let mut request =
        match anthropic_payload_to_chat_request(&payload, &state.config.anthropic_model) {
            Ok(req) => req,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":{"message":format!("invalid request: {err}")}})),
                )
                    .into_response();
            }
        };

    let mut upstream_base_url = state.config.anthropic_base_url.clone();
    let mut upstream_api_key = api_key.clone();
    let mut release_gateway_key: Option<String> = None;

    if !api_key.is_empty() {
        match state.gateway.find_gateway_api_key(&api_key).await {
            Some(gateway_key) => {
                if gateway_key.is_active == 0 {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error":{"message":"api key is inactive"}})),
                    )
                        .into_response();
                }

                if let Some(quota_limit) = gateway_key.quota_limit
                    && gateway_key.used_quota >= quota_limit
                {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({"error":{"message":"api key quota exceeded"}})),
                    )
                        .into_response();
                }

                if let Err(resp) = acquire_runtime_limit(&state, &gateway_key).await {
                    return resp;
                }

                let Some(provider) = state
                    .gateway
                    .select_provider_for_service(&gateway_key.service_id, "anthropic")
                    .await
                else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"no provider bound for service/anthropic"}})),
                    )
                        .into_response();
                };

                let Some((base_url, _)) = resolve_upstream(
                    provider.base_url.clone(),
                    provider.endpoint_id.as_deref(),
                ) else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider base_url missing"}})),
                    )
                        .into_response();
                };
                let Some(provider_api_key) = provider.api_key.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider api_key missing"}})),
                    )
                        .into_response();
                };

                if let Some(mapped_model) =
                    map_model_name(provider.model_mapping.as_deref(), &request.model)
                {
                    request.model = mapped_model;
                }

                upstream_base_url = base_url;
                upstream_api_key = provider_api_key;
                release_gateway_key = Some(gateway_key.key.clone());

                state.gateway.increment_used_quota(&gateway_key.key).await;
            }
            None => {}
        }
    }

    if upstream_api_key.is_empty() {
        upstream_api_key = state.config.anthropic_api_key.clone();
    }
    if upstream_api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"missing upstream api key"}})),
        )
            .into_response();
    }

    match invoke_with_connector(
        UpstreamProtocol::Anthropic,
        &upstream_base_url,
        &upstream_api_key,
        &request,
        None,
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_anthropic_json(&resp);
            let status = StatusCode::OK;
            state
                .gateway
                .record_stat("/v1/messages", 200, start.elapsed().as_millis() as i64)
                .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            state
                .gateway
                .record_stat("/v1/messages", 500, start.elapsed().as_millis() as i64)
                .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err:#}")}})),
            )
                .into_response()
        }
    }
}

async fn acquire_runtime_limit(
    state: &Arc<AppState>,
    gateway_key: &GatewayApiKey,
) -> std::result::Result<(), Response> {
    let mut runtime = state.gateway.api_key_runtime.lock().await;
    let entry = runtime
        .entry(gateway_key.key.clone())
        .or_insert_with(|| RuntimeRateState {
            window_started_at: Instant::now(),
            request_count: 0,
            in_flight: 0,
        });

    if entry.window_started_at.elapsed() >= Duration::from_secs(1) {
        entry.window_started_at = Instant::now();
        entry.request_count = 0;
    }

    if let Some(qps_limit) = gateway_key.qps_limit
        && qps_limit > 0.0
        && (entry.request_count as f64) >= qps_limit
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"message":"api key qps limit exceeded"}})),
        )
            .into_response());
    }

    if let Some(concurrency_limit) = gateway_key.concurrency_limit
        && concurrency_limit > 0
        && (entry.in_flight as i64) >= concurrency_limit
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"message":"api key concurrency limit exceeded"}})),
        )
            .into_response());
    }

    entry.request_count += 1;
    entry.in_flight += 1;

    Ok(())
}

async fn release_runtime_inflight(state: &Arc<AppState>, key: &str) {
    let mut runtime = state.gateway.api_key_runtime.lock().await;
    if let Some(entry) = runtime.get_mut(key)
        && entry.in_flight > 0
    {
        entry.in_flight -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_upstream;

    #[test]
    fn resolve_upstream_minimax_global() {
        let r = resolve_upstream(None, Some("minimax:global"));
        let (url, family) = r.expect("get_endpoint(minimax:global) should return Some");
        assert!(url.contains("minimax"), "base_url should contain minimax: {}", url);
        assert_eq!(family.as_deref(), Some("minimax"));
    }
}
