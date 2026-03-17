use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::Json;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::config::{GatewayApiKey, RuntimeRateState};
use crate::types::AppState;

/// Authenticated gateway key with lifecycle helpers.
pub struct GatewayAuth {
    pub key: GatewayApiKey,
}

impl GatewayAuth {
    /// Authenticate a token: find gateway key → check active → check quota → acquire rate limit.
    /// Returns `Ok(None)` if token doesn't match any key (caller should fall through to env config).
    /// Returns `Err(Response)` on auth failure.
    pub async fn try_authenticate(
        state: &Arc<AppState>,
        token: &str,
    ) -> Result<Option<Self>, Response> {
        if token.is_empty() {
            return Ok(None);
        }
        let Some(gk) = state.gateway.find_gateway_api_key(token).await else {
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
    pub async fn finalize(&self, state: &Arc<AppState>) {
        state.gateway.increment_used_quota(&self.key.key).await;
        release_inflight(state, &self.key.key).await;
    }

    /// Error/cleanup path: release inflight only.
    pub async fn release(&self, state: &Arc<AppState>) {
        release_inflight(state, &self.key.key).await;
    }
}

/// Extract Bearer token from Authorization header, with env fallback.
pub fn extract_bearer_token(headers: &HeaderMap, _env_api_key: &str) -> String {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
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
pub async fn record_stat(state: &Arc<AppState>, endpoint: &str, status_code: u16, start: &Instant) {
    state
        .gateway
        .record_stat(endpoint, status_code, start.elapsed().as_millis() as i64)
        .await;
}

async fn release_inflight(state: &Arc<AppState>, key: &str) {
    let mut runtime = state.gateway.api_key_runtime.lock().await;
    if let Some(entry) = runtime.get_mut(key)
        && entry.in_flight > 0
    {
        entry.in_flight -= 1;
    }
}

async fn acquire_runtime_limit(
    state: &Arc<AppState>,
    gateway_key: &GatewayApiKey,
) -> Result<(), Response> {
    let key = gateway_key.key.clone();
    let qps_limit = gateway_key.qps_limit;
    let concurrency_limit = gateway_key.concurrency_limit;
    {
        let mut runtime = state.gateway.api_key_runtime.lock().await;
        let entry = runtime.entry(key).or_insert_with(|| RuntimeRateState {
            window_started_at: Instant::now(),
            request_count: 0,
            in_flight: 0,
        });

        if entry.window_started_at.elapsed() >= Duration::from_secs(1) {
            entry.window_started_at = Instant::now();
            entry.request_count = 0;
        }

        if let Some(qps) = qps_limit
            && qps > 0.0
            && (entry.request_count as f64) >= qps
        {
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key qps limit exceeded",
            ));
        }

        if let Some(cl) = concurrency_limit
            && cl > 0
            && (entry.in_flight as i64) >= cl
        {
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key concurrency limit exceeded",
            ));
        }

        entry.request_count += 1;
        entry.in_flight += 1;
        Ok(())
    }
}
