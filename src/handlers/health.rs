use axum::{
    extract::{Json, State},
    response::IntoResponse,
};
use serde_json::json;

pub async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","name":"UniGateway","version":"0.2.0"}))
}
