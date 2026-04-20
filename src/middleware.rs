use std::sync::Arc;
use std::time::Instant;

use axum::extract::Json;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::config::{GatewayApiKey, RuntimeLimitError};
use crate::types::GatewayRequestState;

/// Authenticated gateway key with lifecycle helpers.
pub struct GatewayAuth {
    pub key: GatewayApiKey,
}

impl GatewayAuth {
    /// Authenticate a token: find gateway key → check active → check quota → acquire rate limit.
    /// Returns `Ok(None)` if token doesn't match any key (caller should fall through to env config).
    /// Returns `Err(Response)` on auth failure.
    pub async fn try_authenticate(
        state: &Arc<GatewayRequestState>,
        token: &str,
    ) -> Result<Option<Self>, Response> {
        if token.is_empty() {
            // Compatibility fallback for local-only usage:
            // Some clients (e.g. Codex in ChatGPT-auth mode) may omit API key
            // headers even when targeting a local OpenAI-compatible base URL.
            // To keep this safe, only apply implicit auth when:
            // 1) gateway bind address is localhost, and
            // 2) there is exactly one active gateway API key.
            let is_local_bind = state.is_local_bind();

            if is_local_bind {
                let active_keys: Vec<_> = state
                    .gateway()
                    .list_api_keys()
                    .await
                    .into_iter()
                    .filter(|k| k.is_active)
                    .collect();

                if active_keys.len() == 1 {
                    let k = &active_keys[0];
                    let gk = GatewayApiKey {
                        key: k.key.clone(),
                        service_id: k.service_id.clone(),
                        quota_limit: k.quota_limit,
                        used_quota: k.used_quota,
                        is_active: if k.is_active { 1 } else { 0 },
                        qps_limit: k.qps_limit,
                        concurrency_limit: k.concurrency_limit,
                    };

                    if gk.is_active == 0 {
                        return Err(error_json(StatusCode::UNAUTHORIZED, "api key is inactive"));
                    }
                    if let Some(limit) = gk.quota_limit
                        && gk.used_quota >= limit
                    {
                        return Err(error_json(
                            StatusCode::TOO_MANY_REQUESTS,
                            "api key quota exceeded",
                        ));
                    }
                    acquire_runtime_limit(state, &gk).await?;
                    return Ok(Some(Self { key: gk }));
                }
            }

            return Ok(None);
        }
        let Some(gk) = state.gateway().find_gateway_api_key(token).await else {
            return Ok(None);
        };
        if gk.is_active == 0 {
            return Err(error_json(StatusCode::UNAUTHORIZED, "api key is inactive"));
        }
        if let Some(limit) = gk.quota_limit
            && gk.used_quota >= limit
        {
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key quota exceeded",
            ));
        }
        acquire_runtime_limit(state, &gk).await?;
        Ok(Some(Self { key: gk }))
    }

    /// Success path: increment quota + release inflight.
    pub async fn finalize(&self, state: &Arc<GatewayRequestState>) {
        state.gateway().increment_used_quota(&self.key.key).await;
        release_inflight(state, &self.key.key).await;
    }

    /// Error/cleanup path: release inflight only.
    pub async fn release(&self, state: &Arc<GatewayRequestState>) {
        release_inflight(state, &self.key.key).await;
    }
}

/// Extract API key for OpenAI-compatible requests.
///
/// Accept common variants used by different clients:
/// - Authorization: Bearer <key>
/// - api-key: <key>
/// - x-api-key: <key>
pub fn extract_openai_api_key(headers: &HeaderMap, _env_api_key: &str) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_default()
        .chars()
        .collect::<String>()
        .trim()
        .to_string()
}

/// Extract API key from x-api-key header (Anthropic style).
pub fn extract_x_api_key(headers: &HeaderMap, env_api_key: &str) -> String {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        })
        .or_else(|| {
            let _ = env_api_key;
            None
        })
        .unwrap_or_default()
        .chars()
        .collect::<String>()
        .trim()
        .to_string()
}

/// JSON error response helper.
pub fn error_json(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":{"message": message}}))).into_response()
}

/// Record a stat and return the latency.
pub async fn record_stat(
    state: &Arc<GatewayRequestState>,
    endpoint: &str,
    status_code: u16,
    start: &Instant,
) {
    state
        .gateway()
        .record_stat(endpoint, status_code, start.elapsed().as_millis() as i64)
        .await;
}

async fn release_inflight(state: &Arc<GatewayRequestState>, key: &str) {
    state.gateway().release_api_key_inflight(key).await;
}

async fn acquire_runtime_limit(
    state: &Arc<GatewayRequestState>,
    gateway_key: &GatewayApiKey,
) -> Result<(), Response> {
    state
        .gateway()
        .acquire_runtime_limit(gateway_key)
        .await
        .map_err(runtime_limit_error_response)
}

fn runtime_limit_error_response(error: RuntimeLimitError) -> Response {
    match error {
        RuntimeLimitError::QpsWaitTooLong => error_json(
            StatusCode::TOO_MANY_REQUESTS,
            "api key qps wait time too long",
        ),
        RuntimeLimitError::TooManyQpsSleepers => error_json(
            StatusCode::TOO_MANY_REQUESTS,
            "api key qps limit exceeded (too many active requests)",
        ),
        RuntimeLimitError::QueueDepthExceeded => error_json(
            StatusCode::TOO_MANY_REQUESTS,
            "api key concurrency queue depth exceeded",
        ),
        RuntimeLimitError::QueueTimeout => error_json(
            StatusCode::TOO_MANY_REQUESTS,
            "api key request timeout in queue",
        ),
        RuntimeLimitError::StateLost => {
            error_json(StatusCode::INTERNAL_SERVER_ERROR, "api key state lost")
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::{extract_openai_api_key, extract_x_api_key};

    #[test]
    fn anthropic_requests_prefer_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("ugk_anthropic"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-upstream"),
        );

        assert_eq!(extract_x_api_key(&headers, ""), "ugk_anthropic");
    }

    #[test]
    fn anthropic_requests_accept_bearer_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ugk_bearer"),
        );

        assert_eq!(extract_x_api_key(&headers, ""), "ugk_bearer");
    }

    #[test]
    fn openai_requests_accept_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("ugk_openai"));

        assert_eq!(extract_openai_api_key(&headers, ""), "ugk_openai");
    }
}
