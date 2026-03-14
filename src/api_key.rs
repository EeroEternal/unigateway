use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiResponse, CreateApiKeyReq};
use crate::mutations::upsert_api_key_limits;
use crate::queries::list_api_key_out;
use crate::types::AppState;

pub(crate) async fn api_list_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows = list_api_key_out(&state.pool).await;

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

    match upsert_api_key_limits(
        &state.pool,
        &req.key,
        &req.service_id,
        req.quota_limit,
        req.qps_limit,
        req.concurrency_limit,
    )
    .await
    {
        Ok(_) => axum::Json(ApiResponse {
            success: true,
            data: json!({"key": req.key, "service_id": req.service_id}),
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}
