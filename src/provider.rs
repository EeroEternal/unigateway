use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiResponse, CreateProviderReq, ProviderOut};
use crate::types::AppState;

pub(crate) async fn api_list_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ProviderOut> = state
        .gateway
        .list_providers()
        .await
        .into_iter()
        .map(|(id, name, provider_type, endpoint_id, base_url)| ProviderOut {
            id,
            name,
            provider_type,
            endpoint_id,
            base_url,
        })
        .collect();

    axum::Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<CreateProviderReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let provider_id = state
        .gateway
        .create_provider(
            &req.name,
            &req.provider_type,
            &req.endpoint_id,
            req.base_url.as_deref(),
            &req.api_key,
            req.model_mapping.as_deref(),
        )
        .await;
    let _ = state.gateway.persist_if_dirty().await;

    axum::Json(ApiResponse {
        success: true,
        data: json!({"provider_id": provider_id}),
    })
    .into_response()
}

pub(crate) async fn api_bind_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<crate::dto::BindProviderReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state
        .gateway
        .bind_provider_to_service(&req.service_id, req.provider_id)
        .await
    {
        Ok(_) => {
            let _ = state.gateway.persist_if_dirty().await;
            axum::Json(ApiResponse {
                success: true,
                data: json!({"service_id": req.service_id, "provider_id": req.provider_id}),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}
