use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiResponse, ApiKeyOut, CreateApiKeyReq};
use crate::types::AppState;

pub(crate) async fn api_list_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ApiKeyOut> = state
        .gateway
        .list_api_keys()
        .await
        .into_iter()
        .map(|a| ApiKeyOut {
            key: a.key,
            service_id: a.service_id,
            quota_limit: a.quota_limit,
            used_quota: a.used_quota,
            is_active: if a.is_active { 1 } else { 0 },
            qps_limit: a.qps_limit,
            concurrency_limit: a.concurrency_limit,
        })
        .collect();

    axum::Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<CreateApiKeyReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    state
        .gateway
        .create_api_key(
            &req.key,
            &req.service_id,
            req.quota_limit,
            req.qps_limit,
            req.concurrency_limit,
        )
        .await;
    let _ = state.gateway.persist_if_dirty().await;

    axum::Json(ApiResponse {
        success: true,
        data: json!({"key": req.key, "service_id": req.service_id}),
    })
    .into_response()
}
