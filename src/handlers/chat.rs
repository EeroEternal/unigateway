use std::convert::Infallible;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::{Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse},
};
use futures_util::StreamExt;
use llm_connector::types::{ChatRequest, Message, Role};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::server::AppState;

pub async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    handle_chat_request(state, headers, payload, "openai").await
}

pub async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    handle_chat_request(state, headers, payload, "anthropic").await
}

async fn handle_chat_request(
    state: Arc<AppState>,
    headers: HeaderMap,
    payload: Value,
    provider: &str,
) -> Response {
    let start = Instant::now();

    // 1. Extract API Key and Base URL
    let (api_key, base_url) = extract_config(&state, &headers, provider);

    // 2. Build ChatRequest (Use llm-connector types)
    // Note: In a real implementation, we should parse `payload` into `ChatRequest` struct.
    // For now, we assume payload is compatible or we use a helper to convert.
    // Since llm-connector v0.7.0 `ChatRequest` is strict, we might need manual mapping 
    // or use `serde_json::from_value` if the struct derives Deserialize.
    // Here we do a manual mapping for critical fields for demonstration.
    
    let model = payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            if provider == "openai" {
                &state.config.openai_model
            } else {
                &state.config.anthropic_model
            }
        })
        .to_string();

    let messages_vec = payload
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Convert JSON messages to llm-connector Message
    let mut messages = Vec::new();
    for msg in messages_vec {
        if let (Some(role_str), Some(content_str)) = (
            msg.get("role").and_then(|v| v.as_str()),
            msg.get("content").and_then(|v| v.as_str()),
        ) {
            let role = match role_str {
                "system" => Role::System,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => Role::User,
            };
            messages.push(Message::text(role, content_str));
        }
    }

    let mut req = ChatRequest::new(model).with_messages(messages);
    
    // Set max_tokens if present
    if let Some(max_tokens) = payload.get("max_tokens").and_then(|v| v.as_u64()) {
        req.max_tokens = Some(max_tokens as u32);
    }

    // 3. Apply Per-Request Overrides (v0.7.0 Feature)
    if let Some(key) = api_key {
        req = req.with_api_key(key);
    }
    if let Some(url) = base_url {
        req = req.with_base_url(url);
    }
    
    // Inject custom headers if needed (e.g. from upstream client)
    // req = req.with_header("X-Proxy-By", "UniGateway");

    // 4. Handle Streaming vs Non-Streaming
    let is_stream = payload.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if is_stream {
        // --- Streaming Path ---
        match state.engine.client.chat_stream(&req).await {
            Ok(stream) => {
                // Convert llm-connector stream to Axum SSE stream
                let sse_stream = stream.map(|result: Result<_, llm_connector::error::LlmConnectorError>| {
                    match result {
                        Ok(chunk) => {
                            // Map Chunk to SSE Event
                            // OpenAI format: data: {...}
                            // We need to serialize Chunk to JSON string
                            let data = serde_json::to_string(&chunk).unwrap_or_default();
                            Ok::<_, Infallible>(axum::response::sse::Event::default().data(data))
                        }
                        Err(e) => {
                            // On error in stream, we can send a special event or just close
                            // Ideally, we send an error event
                             Ok::<_, Infallible>(axum::response::sse::Event::default().event("error").data(e.to_string()))
                        }
                    }
                });

                // Record stats (initial success) - Stream usage is harder to track perfectly here without wrapping
                record_stat(&state.pool, provider, "/v1/chat/completions", 200, start.elapsed().as_millis() as i64).await;
                
                Sse::new(sse_stream).keep_alive(axum::response::sse::KeepAlive::default()).into_response()
            }
            Err(e) => {
                record_stat(&state.pool, provider, "/v1/chat/completions", 500, start.elapsed().as_millis() as i64).await;
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json_error(e.to_string()))).into_response()
            }
        }
    } else {
        // --- Non-Streaming Path ---
        match state.engine.client.chat(&req).await {
            Ok(resp) => {
                record_stat(&state.pool, provider, "/v1/chat/completions", 200, start.elapsed().as_millis() as i64).await;
                Json(resp).into_response()
            }
            Err(e) => {
                record_stat(&state.pool, provider, "/v1/chat/completions", 500, start.elapsed().as_millis() as i64).await;
                // Here we can map LlmError to specific status codes
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json_error(e.to_string()))).into_response()
            }
        }
    }
}

fn extract_config(
    state: &AppState,
    headers: &HeaderMap,
    provider: &str,
) -> (Option<String>, Option<String>) {
    let api_key = if provider == "openai" {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.replace("Bearer ", ""))
            .or_else(|| {
                if state.config.openai_api_key.is_empty() {
                    None
                } else {
                    Some(state.config.openai_api_key.clone())
                }
            })
    } else {
        headers
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
    };

    let base_url = if provider == "openai" {
        Some(state.config.openai_base_url.clone())
    } else {
        Some(state.config.anthropic_base_url.clone())
    };

    (api_key, base_url)
}

fn json_error(msg: String) -> Value {
    serde_json::json!({
        "error": {
            "message": msg,
            "type": "server_error",
            "code": 500
        }
    })
}

async fn record_stat(
    pool: &SqlitePool,
    provider: &str,
    endpoint: &str,
    status_code: i64,
    latency_ms: i64,
) {
    let _ = sqlx::query(
        "INSERT INTO request_stats(provider, endpoint, status_code, latency_ms) VALUES(?, ?, ?, ?)",
    )
    .bind(provider)
    .bind(endpoint)
    .bind(status_code)
    .bind(latency_ms)
    .execute(pool)
    .await;
}
