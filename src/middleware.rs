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
            // Compatibility fallback for local-only usage:
            // Some clients (e.g. Codex in ChatGPT-auth mode) may omit API key
            // headers even when targeting a local OpenAI-compatible base URL.
            // To keep this safe, only apply implicit auth when:
            // 1) gateway bind address is localhost, and
            // 2) there is exactly one active gateway API key.
            let is_local_bind = state.config.bind.starts_with("127.0.0.1")
                || state.config.bind.starts_with("localhost");

            if is_local_bind {
                let active_keys: Vec<_> = state
                    .gateway
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
pub async fn record_stat(state: &Arc<AppState>, endpoint: &str, status_code: u16, start: &Instant) {
    state
        .gateway
        .record_stat(endpoint, status_code, start.elapsed().as_millis() as i64)
        .await;
}

async fn release_inflight(state: &Arc<AppState>, key: &str) {
    let mut runtime = state.gateway.api_key_runtime.lock().await;
    if let Some(entry) = runtime.get_mut(key) {
        if entry.in_flight > 0 {
            entry.in_flight -= 1;
        }
        if entry.in_queue > 0 {
            entry.notify.notify_one();
        }
    }
}

async fn acquire_runtime_limit(
    state: &Arc<AppState>,
    gateway_key: &GatewayApiKey,
) -> Result<(), Response> {
    let key = gateway_key.key.clone();
    let qps_limit = gateway_key.qps_limit;
    let concurrency_limit = gateway_key.concurrency_limit;
    
    let qps_wait = {
        let mut runtime = state.gateway.api_key_runtime.lock().await;
        let qps = qps_limit.unwrap_or(0.0);
        let entry = runtime.entry(key.clone()).or_insert_with(|| RuntimeRateState {
            last_update: Instant::now(),
            tokens: if qps > 0.0 { (qps * 2.0).max(1.0) } else { 0.0 },
            in_flight: 0,
            in_queue: 0,
            notify: std::sync::Arc::new(tokio::sync::Notify::new()),
        });

        let mut wait = Duration::ZERO;
        if let Some(qps) = qps_limit
            && qps > 0.0
        {
                let now = Instant::now();
                let elapsed = now.duration_since(entry.last_update).as_secs_f64();
                let burst = (qps * 2.0).max(1.0);
                entry.tokens = (entry.tokens + elapsed * qps).min(burst);
                entry.last_update = now;

                if entry.tokens >= 1.0 {
                    entry.tokens -= 1.0;
                } else {
                    let needed = 1.0 - entry.tokens;
                    let wait_secs = needed / qps;
                    wait = Duration::from_secs_f64(wait_secs);
                    if wait <= crate::config::QPS_SHAPING_TIMEOUT {
                        entry.tokens -= 1.0;
                    } else {
                        return Err(error_json(
                            StatusCode::TOO_MANY_REQUESTS,
                            "api key qps wait time too long",
                        ));
                    }
                }
            }
        wait
    };

    if qps_wait > Duration::ZERO {
        let sleepers = crate::config::QPS_SLEEPERS_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if sleepers > crate::config::MAX_QPS_SLEEPERS {
            crate::config::QPS_SLEEPERS_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key qps limit exceeded (too many active requests)",
            ));
        }
        tokio::time::sleep(qps_wait).await;
        crate::config::QPS_SLEEPERS_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }

    let notify = {
        let mut runtime = state.gateway.api_key_runtime.lock().await;
        let Some(entry) = runtime.get_mut(&key) else {
            return Err(error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api key state evicted unexpectedly",
            ));
        };
        
        if let Some(cl) = concurrency_limit {
            if cl > 0 && (entry.in_flight as i64) >= cl {
                if entry.in_queue >= crate::config::MAX_QUEUE_PER_KEY {
                    return Err(error_json(
                        StatusCode::TOO_MANY_REQUESTS,
                        "api key concurrency queue depth exceeded",
                    ));
                }
                entry.in_queue += 1;
                entry.notify.clone()
            } else {
                entry.in_flight += 1;
                return Ok(());
            }
        } else {
            entry.in_flight += 1;
            return Ok(());
        }
    };

    // We reached here, meaning we are queued
    let start = Instant::now();
    let timeout_dur = crate::config::CONCURRENCY_QUEUE_TIMEOUT;

    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout_dur {
            let mut runtime = state.gateway.api_key_runtime.lock().await;
            if let Some(entry) = runtime.get_mut(&key)
                && entry.in_queue > 0
            {
                entry.in_queue -= 1;
            }
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key request timeout in queue",
            ));
        }

        let wait_fut = tokio::time::timeout(timeout_dur - elapsed, notify.notified());
        if wait_fut.await.is_err() {
            let mut runtime = state.gateway.api_key_runtime.lock().await;
            if let Some(entry) = runtime.get_mut(&key)
                && entry.in_queue > 0
            {
                entry.in_queue -= 1;
            }
            return Err(error_json(
                StatusCode::TOO_MANY_REQUESTS,
                "api key request timeout in queue",
            ));
        }

        let mut runtime = state.gateway.api_key_runtime.lock().await;
        if let Some(entry) = runtime.get_mut(&key) {
            if let Some(cl) = concurrency_limit {
                if (entry.in_flight as i64) < cl {
                    if entry.in_queue > 0 {
                        entry.in_queue -= 1;
                    }
                    entry.in_flight += 1;
                    return Ok(());
                }
            } else {
                if entry.in_queue > 0 {
                    entry.in_queue -= 1;
                }
                entry.in_flight += 1;
                return Ok(());
            }
        } else {
            return Err(error_json(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api key state lost",
            ));
        }
    }
}
