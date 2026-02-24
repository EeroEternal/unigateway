use std::sync::Arc;

use axum::{
    extract::{Form, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ui;

use super::{
    auth::ensure_login,
    types::{AppState, ModelItem, ModelList},
};

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

    if headers.contains_key("hx-request") {
        Html(ui::templates::ADMIN_PAGE.to_string()).into_response()
    } else {
        Html(ui::admin_page()).into_response()
    }
}

pub(crate) async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::ADMIN_PAGE.to_string()).into_response()
    } else {
        Html(ui::admin_page()).into_response()
    }
}

pub(crate) async fn admin_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::PROVIDERS_PAGE.to_string()).into_response()
    } else {
        Html(ui::providers_page()).into_response()
    }
}

pub(crate) async fn admin_api_keys_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::KEYS_PAGE.to_string()).into_response()
    } else {
        Html(ui::keys_page()).into_response()
    }
}

pub(crate) async fn admin_logs_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::LOGS_PAGE.to_string()).into_response()
    } else {
        Html(ui::logs_page()).into_response()
    }
}

pub(crate) async fn admin_settings_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::SETTINGS_PAGE.to_string()).into_response()
    } else {
        Html(ui::settings_page()).into_response()
    }
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

pub(crate) async fn admin_providers_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let providers: Vec<(i64, String, String, Option<String>)> =
        sqlx::query_as("SELECT id, name, provider_type, base_url FROM providers ORDER BY id")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut rows_html = String::new();
    for (id, name, ptype, url) in providers {
        let first_char = name.chars().next().unwrap_or('?');
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                  <div class='w-8 h-8 bg-slate-100 rounded-lg flex items-center justify-center text-slate-400 font-bold group-hover:bg-brand group-hover:text-white transition-all uppercase text-[11px]'>
                      {}
                  </div>
                  <span class='font-bold text-slate-700 text-sm tracking-tight'>{}</span>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1.5 rounded-md text-[10px] uppercase tracking-widest h-auto shadow-none'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <code class='text-[12px] font-mono text-slate-400 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  hx-delete='/admin/providers/{}'
                  hx-target='#providers-list'
                  hx-confirm='确定要移除该供应商吗？'
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  移除
                </button>
              </td>
            </tr>",
            first_char, name, ptype, url.unwrap_or_default(), id
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='4' class='text-center py-20 text-slate-300 font-bold'>暂无模型供应商</td></tr>");
    }

    let final_html = ui::templates::PROVIDERS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_api_keys_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let keys: Vec<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT key, name, used_quota FROM api_keys ORDER BY created_at DESC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut rows_html = String::new();
    for (key, name, used) in keys {
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                    <div class='w-8 h-8 rounded-lg bg-slate-50 border border-slate-100 flex items-center justify-center text-slate-400 transition-all group-hover:bg-brand/10 group-hover:border-brand/20 group-hover:text-brand'>
                        <svg xmlns='http://www.w3.org/2000/svg' class='h-4 w-4' fill='none' viewBox='0 0 24 24' stroke='currentColor' stroke-width='2.5'><path stroke-linecap='round' stroke-linejoin='round' d='M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z' /></svg>
                    </div>
                    <div class='flex flex-col'>
                        <code class='text-[13px] font-mono font-bold text-slate-700 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight'>{}</code>
                        <span class='text-[11px] text-slate-500 font-medium mt-1 ml-1'>{}</span>
                    </div>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-2'>
                    <div class='w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse'></div>
                    <span class='text-[11px] font-bold uppercase tracking-widest text-slate-600'>Active</span>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[12px] font-bold text-slate-400 uppercase tracking-widest'>${:.4} used</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  hx-delete='/admin/api-keys/{}'
                  hx-target='#keys-list'
                  hx-confirm='确定要作废该令牌吗？'
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  作废
                </button>
              </td>
            </tr>",
            key, name.unwrap_or_default(), (used as f64) / 1000.0, key
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='4' class='text-center py-20 text-slate-300 font-bold'>暂无 API 令牌</td></tr>");
    }

    let final_html = ui::templates::KEYS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_logs_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let logs: Vec<(
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT created_at, model, status_code, latency_ms, service_id
         FROM request_logs ORDER BY id DESC LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut rows_html = String::new();
    for (created_at, model, status, latency, service_id) in logs {
        let status_val = status.unwrap_or(0);
        let status_class = if status_val < 300 {
            "bg-emerald-50 text-emerald-600 border-emerald-100"
        } else {
            "bg-rose-50 text-rose-600 border-rose-100"
        };
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <code class='text-[12px] font-mono font-bold text-slate-700 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md tracking-tight'>/v1/chat/...</code>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge {} border font-bold px-2 py-1 rounded-md text-[10px] h-auto shadow-none'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-500'>{}ms</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-slate-50 border-slate-200 text-slate-400 font-bold px-2 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none'>{}</span>
              </td>
            </tr>",
            created_at, status_class, status_val, latency.unwrap_or(0), service_id.unwrap_or_default()
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='5' class='text-center py-20 text-slate-300 font-bold'>暂无日志记录</td></tr>");
    }

    Html(rows_html).into_response()
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

    render_providers_partial(&providers).into_response()
}

pub(crate) async fn admin_create_provider_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<CreateProviderForm>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if !form.name.trim().is_empty() && !form.base_url.trim().is_empty() {
        let _ = sqlx::query(
            "INSERT INTO providers(name, provider_type, base_url, api_key, model_mapping, is_enabled)
             VALUES(?, ?, ?, ?, ?, 1)",
        )
        .bind(form.name.trim())
        .bind(form.provider_type.trim())
        .bind(form.base_url.trim())
        .bind(form.api_key.trim())
        .bind(form.model_mapping.unwrap_or_default())
        .execute(&state.pool)
        .await;
    }

    admin_providers_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_bind_provider_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<BindProviderForm>,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if !form.service_id.trim().is_empty() {
        if let Ok(provider_id) = form.provider_id.trim().parse::<i64>() {
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)",
            )
            .bind(form.service_id.trim())
            .bind(provider_id)
            .execute(&state.pool)
            .await;
        }
    }

    let providers: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, provider_type, base_url FROM providers WHERE COALESCE(is_enabled, 1)=1 ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    render_providers_partial(&providers).into_response()
}

fn render_providers_partial(providers: &[(i64, String, String, Option<String>)]) -> Html<String> {
    let mut rows = String::new();
    for (id, name, provider_type, base_url) in providers {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            id,
            name,
            provider_type,
            base_url.clone().unwrap_or_default()
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='4' class='text-base-content/60'>暂无 Provider</td></tr>");
    }

    Html(format!(
        "<div class='card bg-base-100 shadow'><div class='card-body space-y-4'><h3 class='card-title text-base'>Providers</h3><form class='grid grid-cols-1 md:grid-cols-5 gap-2' hx-post='/admin/providers/create' hx-target='#providers-box' hx-swap='outerHTML'><input class='input input-bordered input-sm' name='name' placeholder='name' /><input class='input input-bordered input-sm' name='provider_type' placeholder='type: openai/anthropic' /><input class='input input-bordered input-sm' name='base_url' placeholder='base url' /><input class='input input-bordered input-sm' name='api_key' placeholder='api key' /><button class='btn btn-primary btn-sm' type='submit'>新增 Provider</button><input class='input input-bordered input-sm md:col-span-4' name='model_mapping' placeholder='model mapping (json or model)' /></form><form class='grid grid-cols-1 md:grid-cols-3 gap-2' hx-post='/admin/providers/bind' hx-target='#providers-box' hx-swap='outerHTML'><input class='input input-bordered input-sm' name='service_id' placeholder='service id' /><input class='input input-bordered input-sm' name='provider_id' placeholder='provider id' /><button class='btn btn-secondary btn-sm' type='submit'>绑定 Provider</button></form><div class='overflow-x-auto'><table class='table table-sm'><thead><tr><th>ID</th><th>Name</th><th>Type</th><th>Base URL</th></tr></thead><tbody>{}</tbody></table></div></div></div>",
        rows
    ))
}

pub(crate) async fn admin_providers_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let _ = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    admin_providers_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_api_keys_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(key): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let _ = sqlx::query("DELETE FROM api_keys WHERE key = ?")
        .bind(key)
        .execute(&state.pool)
        .await;

    admin_api_keys_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_create_api_key_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<CreateApiKeyForm>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if !form.name.trim().is_empty() {
        let key = format!("sk-{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let _ = sqlx::query(
            "INSERT INTO api_keys(name, key, service_id, quota_limit, used_quota, is_active) VALUES(?, ?, 'default', ?, 0, 1)",
        )
        .bind(form.name.trim())
        .bind(key)
        .bind(form.quota_limit)
        .execute(&state.pool)
        .await;
    }

    admin_api_keys_list_partial(State(state), headers)
        .await
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

#[derive(Deserialize)]
pub(crate) struct CreateServiceForm {
    id: String,
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateProviderForm {
    name: String,
    provider_type: String,
    base_url: String,
    api_key: String,
    model_mapping: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BindProviderForm {
    service_id: String,
    provider_id: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateApiKeyForm {
    name: String,
    quota_limit: i64,
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

    let rows: Vec<ProviderOut> =
        sqlx::query_as("SELECT id, name, provider_type, base_url FROM providers ORDER BY id DESC")
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

    let result = sqlx::query(
        "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)",
    )
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

fn parse_optional_i64(value: Option<&str>) -> Option<i64> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<i64>().ok()
        }
    })
}

fn parse_optional_f64(value: Option<&str>) -> Option<f64> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<f64>().ok()
        }
    })
}
