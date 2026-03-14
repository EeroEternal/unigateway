use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::types::{AppState, ModelItem, ModelList};
use crate::queries::fetch_metrics_snapshot;

pub(crate) async fn health() -> impl IntoResponse {
    axum::Json(json!({"status":"ok","name":"UniGateway"}))
}

pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snapshot = fetch_metrics_snapshot(&state.pool).await;

    let body = format!(
        "# TYPE unigateway_requests_total counter\nunigateway_requests_total {}\n# TYPE unigateway_requests_by_endpoint_total counter\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}\n",
        snapshot.total, snapshot.openai_total, snapshot.anthropic_total
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
