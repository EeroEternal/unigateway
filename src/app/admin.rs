use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ui;

use super::{auth::ensure_login, types::{AppState, ModelItem, ModelList}};

pub(crate) async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","name":"UniGateway"}))
}

pub(crate) async fn home() -> impl IntoResponse {
    Redirect::to("/admin")
}

pub(crate) async fn admin_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }

    if !ensure_login(&state.pool, &headers).await {
        return Redirect::to("/login").into_response();
    }

    Html(ui::admin_page()).into_response()
}

pub(crate) async fn admin_stats_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }

    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let openai_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let anthropic_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let content = ui::stats_partial(total, openai_count, anthropic_count);

    Html(content).into_response()
}

pub(crate) async fn admin_services_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let services: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM services ORDER BY id")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let mut rows = String::new();
    for (id, name) in services {
        rows.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", id, name));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='2' class='text-base-content/60'>暂无服务</td></tr>");
    }

    Html(format!(
        "<div class='card bg-base-100 shadow'><div class='card-body'><h3 class='card-title text-base'>Services</h3><div class='overflow-x-auto'><table class='table table-sm'><thead><tr><th>ID</th><th>Name</th></tr></thead><tbody>{}</tbody></table></div></div></div>",
        rows
    ))
    .into_response()
}

pub(crate) async fn admin_providers_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let providers: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, provider_type, base_url FROM providers WHERE COALESCE(is_enabled, 1)=1 ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut rows = String::new();
    for (id, name, provider_type, base_url) in providers {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            id,
            name,
            provider_type,
            base_url.unwrap_or_default()
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='4' class='text-base-content/60'>暂无 Provider</td></tr>");
    }

    Html(format!(
        "<div class='card bg-base-100 shadow'><div class='card-body'><h3 class='card-title text-base'>Providers</h3><div class='overflow-x-auto'><table class='table table-sm'><thead><tr><th>ID</th><th>Name</th><th>Type</th><th>Base URL</th></tr></thead><tbody>{}</tbody></table></div></div></div>",
        rows
    ))
    .into_response()
}

pub(crate) async fn admin_api_keys_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let keys: Vec<(String, String, Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT key, service_id, quota_limit, COALESCE(used_quota,0), COALESCE(is_active,1) FROM api_keys ORDER BY created_at DESC LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut rows = String::new();
    for (key, service_id, quota_limit, used_quota, is_active) in keys {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td></tr>",
            mask_key(&key),
            service_id,
            used_quota,
            quota_limit.map(|v| v.to_string()).unwrap_or_else(|| "∞".to_string()),
            if is_active == 1 { "active" } else { "inactive" }
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='4' class='text-base-content/60'>暂无 API Key</td></tr>");
    }

    Html(format!(
        "<div class='card bg-base-100 shadow'><div class='card-body'><h3 class='card-title text-base'>API Keys</h3><div class='overflow-x-auto'><table class='table table-sm'><thead><tr><th>Key</th><th>Service</th><th>Quota</th><th>Status</th></tr></thead><tbody>{}</tbody></table></div></div></div>",
        rows
    ))
    .into_response()
}

pub(crate) async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let openai_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let anthropic_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let body = format!(
        "# TYPE unigateway_requests_total counter\nunigateway_requests_total {}\n# TYPE unigateway_requests_by_endpoint_total counter\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}\n",
        total, openai_total, anthropic_total
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

#[derive(Serialize)]
pub(crate) struct ApiResponse<T: Serialize> {
    success: bool,
    data: T,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ServiceOut {
    id: String,
    name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ProviderOut {
    id: i64,
    name: String,
    provider_type: String,
    base_url: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ApiKeyOut {
    key: String,
    service_id: String,
    quota_limit: Option<i64>,
    used_quota: i64,
    is_active: i64,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct CreateServiceReq {
    id: String,
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateProviderReq {
    name: String,
    provider_type: String,
    base_url: String,
    api_key: String,
    model_mapping: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BindProviderReq {
    service_id: String,
    provider_id: i64,
}

#[derive(Deserialize)]
pub(crate) struct CreateApiKeyReq {
    key: String,
    service_id: String,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
}

pub(crate) async fn api_list_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ServiceOut> = sqlx::query_as("SELECT id, name FROM services ORDER BY id")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

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

    let result = sqlx::query("INSERT OR REPLACE INTO services(id, name) VALUES(?, ?)")
        .bind(&req.id)
        .bind(&req.name)
        .execute(&state.pool)
        .await;

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

    let rows: Vec<ProviderOut> = sqlx::query_as(
        "SELECT id, name, provider_type, base_url FROM providers ORDER BY id DESC",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

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

    let result = sqlx::query(
        "INSERT INTO providers(name, provider_type, base_url, api_key, model_mapping, is_enabled)
         VALUES(?, ?, ?, ?, ?, 1)",
    )
    .bind(&req.name)
    .bind(&req.provider_type)
    .bind(&req.base_url)
    .bind(&req.api_key)
    .bind(req.model_mapping.as_deref().unwrap_or(""))
    .execute(&state.pool)
    .await;

    match result {
        Ok(r) => Json(ApiResponse {
            success: true,
            data: json!({"provider_id": r.last_insert_rowid()}),
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

    let result = sqlx::query("INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)")
        .bind(&req.service_id)
        .bind(req.provider_id)
        .execute(&state.pool)
        .await;

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

    let rows: Vec<ApiKeyOut> = sqlx::query_as(
        "SELECT
            k.key,
            k.service_id,
            k.quota_limit,
            COALESCE(k.used_quota, 0) AS used_quota,
            COALESCE(k.is_active, 1) AS is_active,
            l.qps_limit,
            l.concurrency_limit
         FROM api_keys k
         LEFT JOIN api_key_limits l ON l.api_key = k.key
         ORDER BY k.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

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

    let result1 = sqlx::query(
        "INSERT OR REPLACE INTO api_keys(key, service_id, quota_limit, used_quota, is_active)
         VALUES(?, ?, ?, COALESCE((SELECT used_quota FROM api_keys WHERE key = ?), 0), 1)",
    )
    .bind(&req.key)
    .bind(&req.service_id)
    .bind(req.quota_limit)
    .bind(&req.key)
    .execute(&state.pool)
    .await;

    if let Err(e) = result1 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": e.to_string()})),
        )
            .into_response();
    }

    let result2 = sqlx::query(
        "INSERT OR REPLACE INTO api_key_limits(api_key, qps_limit, concurrency_limit)
         VALUES(?, ?, ?)",
    )
    .bind(&req.key)
    .bind(req.qps_limit)
    .bind(req.concurrency_limit)
    .execute(&state.pool)
    .await;

    match result2 {
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

async fn is_admin_authorized(state: &Arc<AppState>, headers: &HeaderMap) -> bool {
    if !state.config.admin_token.is_empty() {
        let token = headers
            .get("x-admin-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        return token == state.config.admin_token;
    }

    if state.config.enable_ui {
        return ensure_login(&state.pool, headers).await;
    }

    true
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return key.to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}
