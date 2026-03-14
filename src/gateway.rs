use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use llm_providers::get_endpoint;
use serde_json::json;

use crate::protocol::{
    UpstreamProtocol, anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, invoke_with_connector, openai_payload_to_chat_request,
};
use crate::storage::{
    find_gateway_api_key, map_model_name, record_stat, select_provider_for_service,
};
use crate::types::{AppState, GatewayApiKey, RuntimeRateState};

fn resolve_upstream_base_url(
    provider_base_url: Option<String>,
    endpoint_id: Option<&str>,
) -> Option<String> {
    if let Some(url) = provider_base_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let endpoint_id = endpoint_id?.trim();
    if endpoint_id.is_empty() {
        return None;
    }

    let (_family_id, endpoint) = get_endpoint(endpoint_id)?;
    Some(endpoint.base_url.to_string())
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
    let mut provider_label = "openai".to_string();
    let mut release_gateway_key: Option<String> = None;

    if !token.is_empty() {
        match find_gateway_api_key(&state.pool, token).await {
            Ok(Some(gateway_key)) => {
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

                let provider =
                    match select_provider_for_service(&state, &gateway_key.service_id, "openai")
                        .await
                    {
                        Ok(Some(provider)) => provider,
                        Ok(None) => {
                            release_runtime_inflight(&state, &gateway_key.key).await;
                            return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(
                                json!({"error":{"message":"no provider bound for service/openai"}}),
                            ),
                        )
                            .into_response();
                        }
                        Err(err) => {
                            release_runtime_inflight(&state, &gateway_key.key).await;
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(json!({"error":{"message":format!("db error: {err}")}})),
                            )
                                .into_response();
                        }
                    };

                let Some(base_url) = resolve_upstream_base_url(
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
                provider_label = provider.name;
                release_gateway_key = Some(gateway_key.key.clone());

                let _ = sqlx::query(
                    "UPDATE api_keys SET used_quota = COALESCE(used_quota, 0) + 1 WHERE key = ?",
                )
                .bind(&gateway_key.key)
                .execute(&state.pool)
                .await;
            }
            Ok(None) => {}
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":{"message":format!("db error: {err}")}})),
                )
                    .into_response();
            }
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

    match invoke_with_connector(
        UpstreamProtocol::OpenAi,
        &upstream_base_url,
        &upstream_api_key,
        &request,
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_openai_json(&resp);
            let status = StatusCode::OK;
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/chat/completions",
                200,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/chat/completions",
                500,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err}")}})),
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
    let mut provider_label = "anthropic".to_string();
    let mut release_gateway_key: Option<String> = None;

    if !api_key.is_empty() {
        match find_gateway_api_key(&state.pool, &api_key).await {
            Ok(Some(gateway_key)) => {
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

                let provider = match select_provider_for_service(
                    &state,
                    &gateway_key.service_id,
                    "anthropic",
                )
                .await
                {
                    Ok(Some(provider)) => provider,
                    Ok(None) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":{"message":"no provider bound for service/anthropic"}})),
                        )
                            .into_response();
                    }
                    Err(err) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error":{"message":format!("db error: {err}")}})),
                        )
                            .into_response();
                    }
                };

                let Some(base_url) = resolve_upstream_base_url(
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
                provider_label = provider.name;
                release_gateway_key = Some(gateway_key.key.clone());

                let _ = sqlx::query(
                    "UPDATE api_keys SET used_quota = COALESCE(used_quota, 0) + 1 WHERE key = ?",
                )
                .bind(&gateway_key.key)
                .execute(&state.pool)
                .await;
            }
            Ok(None) => {}
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":{"message":format!("db error: {err}")}})),
                )
                    .into_response();
            }
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
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_anthropic_json(&resp);
            let status = StatusCode::OK;
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/messages",
                200,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/messages",
                500,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err}")}})),
            )
                .into_response()
        }
    }
}

async fn acquire_runtime_limit(
    state: &Arc<AppState>,
    gateway_key: &GatewayApiKey,
) -> std::result::Result<(), Response> {
    let mut runtime = state.api_key_runtime.lock().await;
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
    let mut runtime = state.api_key_runtime.lock().await;
    if let Some(entry) = runtime.get_mut(key)
        && entry.in_flight > 0
    {
        entry.in_flight -= 1;
    }
}
