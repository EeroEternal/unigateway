use std::sync::Arc;

use axum::{
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde_json::json;

use crate::types::{AppState, ModelItem, ModelList};

pub(crate) async fn health() -> impl IntoResponse {
    axum::Json(json!({"status":"ok","name":"UniGateway"}))
}

pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (total, openai_total, anthropic_total, embeddings_total) =
        state.gateway.metrics_snapshot().await;

    let body = format!(
        "# TYPE unigateway_requests_total counter\nunigateway_requests_total {}\n# TYPE unigateway_requests_by_endpoint_total counter\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/embeddings\"}} {}\n",
        total, openai_total, anthropic_total, embeddings_total
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

pub(crate) async fn models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    axum::Json(ModelList {
        object: "list",
        data: vec![
            ModelItem {
                id: state.config.openai_model.clone(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "openai",
            },
            ModelItem {
                id: state.config.anthropic_model.clone(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "anthropic",
            },
        ],
    })
}

#[derive(serde::Serialize)]
pub struct QueueMetrics {
    pub sleepers_count: usize,
    pub api_keys: std::collections::HashMap<String, ApiKeyMetrics>,
    pub aimd: std::collections::HashMap<String, unigateway_core::engine::AimdSnapshot>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyMetrics {
    pub tokens: f64,
    pub in_flight: u64,
    pub in_queue: u64,
}

pub(crate) async fn queue_metrics(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    use crate::authz::is_admin_authorized;
    if !is_admin_authorized(&state, &headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(json!({"error": "Unauthorized endpoint"})),
        )
            .into_response();
    }

    let sleepers_count = crate::config::QPS_SLEEPERS_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let mut api_keys = std::collections::HashMap::new();
    let guard = state.gateway.api_key_runtime.lock().await;

    for (key, entry) in guard.iter() {
        let char_count = key.chars().count();
        let masked_key = if char_count > 12 {
            let first_chars: String = key.chars().take(3).collect();
            let last_chars: String = key.chars().skip(char_count - 4).collect();
            format!("{}****{}", first_chars, last_chars)
        } else {
            "****".to_string()
        };

        api_keys.insert(
            masked_key,
            ApiKeyMetrics {
                tokens: entry.tokens,
                in_flight: entry.in_flight,
                in_queue: entry.in_queue,
            },
        );
    }

    (
        axum::http::StatusCode::OK,
        axum::Json(QueueMetrics {
            sleepers_count,
            api_keys,
            aimd: state.core_engine.aimd_metrics().await,
        }),
    ).into_response()
}
