use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::authz::is_admin_authorized;

use super::AdminState;

#[derive(serde::Serialize)]
pub struct QueueMetrics {
    pub sleepers_count: usize,
    pub api_keys: HashMap<String, ApiKeyMetrics>,
    pub aimd: HashMap<String, unigateway_core::engine::AimdSnapshot>,
}

#[derive(serde::Serialize)]
pub struct ApiKeyMetrics {
    pub tokens: f64,
    pub in_flight: u64,
    pub in_queue: u64,
}

pub(crate) async fn queue_metrics(
    State(state): State<Arc<AdminState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(json!({"error": "Unauthorized endpoint"})),
        )
            .into_response();
    }

    let sleepers_count =
        crate::config::QPS_SLEEPERS_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    let mut api_keys = HashMap::new();
    let snapshots = state.gateway().queue_metrics_snapshot().await;

    for (key, entry) in snapshots {
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
            aimd: state.aimd_metrics().await,
        }),
    )
        .into_response()
}
