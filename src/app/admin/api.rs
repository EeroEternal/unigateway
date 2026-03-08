use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use serde_json::json;

use crate::app::types::{AppState, ModelItem, ModelList};

use super::{
    authz::is_admin_authorized,
    dto::{
        ApiKeyOut, ApiResponse, BindProviderReq, CreateApiKeyReq, CreateProviderReq,
        CreateServiceReq, ProviderOut, ServiceOut,
    },
    mutations::{bind_provider_to_service, create_provider, upsert_api_key_limits, upsert_service},
    queries::{fetch_metrics_snapshot, list_api_key_out, list_provider_out, list_service_out},
};

pub(crate) async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","name":"UniGateway"}))
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
    Json(ModelList {
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

pub(crate) async fn api_list_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ServiceOut> = list_service_out(&state.pool).await;

    Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateServiceReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let result = upsert_service(&state.pool, &req.id, &req.name).await;

    match result {
        Ok(_) => Json(ApiResponse {
            success: true,
            data: json!({"id": req.id, "name": req.name}),
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) async fn api_list_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ProviderOut> = list_provider_out(&state.pool).await;

    Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateProviderReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let result = create_provider(
        &state.pool,
        &req.name,
        &req.provider_type,
        &req.endpoint_id,
        req.base_url.as_deref(),
        &req.api_key,
        req.model_mapping.as_deref(),
    )
    .await;

    match result {
        Ok(provider_id) => Json(ApiResponse {
            success: true,
            data: json!({"provider_id": provider_id}),
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) async fn api_bind_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BindProviderReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let result = bind_provider_to_service(&state.pool, &req.service_id, req.provider_id).await;

    match result {
        Ok(_) => Json(ApiResponse {
            success: true,
            data: json!({"service_id": req.service_id, "provider_id": req.provider_id}),
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) async fn api_list_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ApiKeyOut> = list_api_key_out(&state.pool).await;

    Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateApiKeyReq>,
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
        Ok(_) => Json(ApiResponse {
            success: true,
            data: json!({"key": req.key, "service_id": req.service_id}),
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}
