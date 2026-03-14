use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiResponse, CreateServiceReq, ServiceOut};
use crate::types::AppState;

pub(crate) async fn api_list_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ServiceOut> = state
        .gateway
        .list_services()
        .await
        .into_iter()
        .map(|(id, name)| ServiceOut { id, name })
        .collect();

    axum::Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<CreateServiceReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    state.gateway.create_service(&req.id, &req.name).await;
    let _ = state.gateway.persist_if_dirty().await;

    axum::Json(ApiResponse {
        success: true,
        data: json!({"id": req.id, "name": req.name}),
    })
    .into_response()
}
