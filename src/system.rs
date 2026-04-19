use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use unigateway_host::env::EnvProvider;

use crate::types::{ModelItem, ModelList, SystemState};

pub(crate) fn router() -> Router<Arc<SystemState>> {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/models", get(models))
}

pub(crate) async fn health() -> impl IntoResponse {
    axum::Json(json!({"status":"ok","name":"UniGateway"}))
}

pub(crate) async fn metrics(State(state): State<Arc<SystemState>>) -> impl IntoResponse {
    let (total, openai_total, anthropic_total, embeddings_total) =
        state.gateway().metrics_snapshot().await;

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

pub(crate) async fn models(State(state): State<Arc<SystemState>>) -> impl IntoResponse {
    axum::Json(ModelList {
        object: "list",
        data: vec![
            ModelItem {
                id: state.provider_model(EnvProvider::OpenAi).to_string(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "openai",
            },
            ModelItem {
                id: state.provider_model(EnvProvider::Anthropic).to_string(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "anthropic",
            },
        ],
    })
}
