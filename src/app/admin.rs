use std::sync::Arc;

use axum::{
    extract::{Form, Query, RawForm, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
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

pub(crate) async fn admin_provider_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let provider_row: Option<(i64, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, name, provider_type, endpoint_id, base_url FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((provider_id, provider_name, provider_type, endpoint_id, base_url)) = provider_row else {
        return (StatusCode::NOT_FOUND, Html("Provider not found".to_string())).into_response();
    };

    let bound_services: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT s.id, s.name, s.created_at
         FROM service_providers sp
         JOIN services s ON s.id = sp.service_id
         WHERE sp.provider_id = ?
         ORDER BY s.created_at DESC",
    )
    .bind(provider_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut service_rows = String::new();
    for (service_id, service_name, created_at) in bound_services {
        service_rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100'>
                <button onclick='openServiceDetail(&quot;{}&quot;)' class='font-semibold text-slate-800 hover:text-teal-800 transition-colors'>{}</button>
              </td>
              <td class='py-4 px-6 border-b border-slate-100'><code class='text-[12px] font-mono text-slate-600 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code></td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm font-semibold text-slate-500'>{}</td>
            </tr>",
            service_id, service_name, service_id, created_at
        ));
    }
    if service_rows.is_empty() {
        service_rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No bound services</td></tr>");
    }

    let body = ui::templates::PROVIDER_DETAIL_PAGE
        .replace("{{provider_name}}", &provider_name)
        .replace("{{provider_type}}", &provider_type)
        .replace("{{endpoint_id}}", &endpoint_id.unwrap_or_else(|| "-".to_string()))
        .replace("{{base_url}}", &base_url.unwrap_or_else(|| "-".to_string()))
        .replace("{{service_rows}}", &service_rows);

    if headers.contains_key("hx-request") {
        Html(body).into_response()
    } else {
        Html(ui::provider_detail_page(&body)).into_response()
    }
}

pub(crate) async fn admin_service_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let service_row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id, name, created_at FROM services WHERE id = ?",
    )
    .bind(&service_id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((service_id, service_name, created_at)) = service_row else {
        return (StatusCode::NOT_FOUND, Html("Service not found".to_string())).into_response();
    };

    let providers: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT p.name, p.provider_type, p.endpoint_id
         FROM service_providers sp
         JOIN providers p ON p.id = sp.provider_id
         WHERE sp.service_id = ?
         ORDER BY p.name ASC",
    )
    .bind(&service_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let api_keys: Vec<(Option<String>, String, String)> = sqlx::query_as(
        "SELECT name, key, created_at FROM api_keys WHERE service_id = ? ORDER BY created_at DESC",
    )
    .bind(&service_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut provider_rows = String::new();
    for (name, provider_type, endpoint_id) in providers {
        provider_rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100 font-semibold text-slate-800'>{}</td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
            </tr>",
            name, provider_type, endpoint_id
        ));
    }
    if provider_rows.is_empty() {
        provider_rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No providers bound</td></tr>");
    }

    let mut api_key_rows = String::new();
    for (name, key, created_at) in api_keys {
        api_key_rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100'><button onclick='openApiKeyDetail(&quot;{}&quot;)' class='font-semibold text-slate-800 hover:text-teal-800 transition-colors'>{}</button></td>
              <td class='py-4 px-6 border-b border-slate-100'><code class='text-[12px] font-mono text-slate-600'>{}</code></td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
            </tr>",
            key,
            name.unwrap_or_default(),
            key,
            created_at
        ));
    }
    if api_key_rows.is_empty() {
        api_key_rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No API keys</td></tr>");
    }

    let body = ui::templates::SERVICE_DETAIL_PAGE
        .replace("{{service_name}}", &service_name)
        .replace("{{service_id}}", &service_id)
        .replace("{{created_at}}", &created_at)
        .replace("{{provider_rows}}", &provider_rows)
        .replace("{{api_key_rows}}", &api_key_rows);

    if headers.contains_key("hx-request") {
        Html(body).into_response()
    } else {
        Html(ui::service_detail_page(&body)).into_response()
    }
}

pub(crate) async fn admin_api_key_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(api_key): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let row: Option<(Option<String>, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT k.name, k.key, k.service_id, s.name, k.created_at
         FROM api_keys k
         LEFT JOIN services s ON s.id = k.service_id
         WHERE k.key = ?",
    )
    .bind(&api_key)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or(None);

    let Some((name, key, service_id, service_name, created_at)) = row else {
        return (StatusCode::NOT_FOUND, Html("API key not found".to_string())).into_response();
    };

    let service_name = service_name.unwrap_or_else(|| {
        if service_id == "default" {
            "Default Service".to_string()
        } else {
            service_id.clone()
        }
    });

    let body = ui::templates::API_KEY_DETAIL_PAGE
        .replace("{{api_key_name}}", &name.unwrap_or_default())
        .replace("{{api_key_value}}", &key)
        .replace("{{created_at}}", &created_at)
        .replace("{{service_id}}", &service_id)
        .replace("{{service_name}}", &service_name);

    if headers.contains_key("hx-request") {
        Html(body).into_response()
    } else {
        Html(ui::api_key_detail_page(&body)).into_response()
    }
}

pub(crate) async fn admin_services_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if headers.contains_key("hx-request") {
        Html(ui::templates::SERVICES_PAGE.to_string()).into_response()
    } else {
        Html(ui::services_page()).into_response()
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
        Html(ui::render_providers_body()).into_response()
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

    let providers: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, provider_type FROM providers ORDER BY id DESC")
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut provider_options = String::new();
    for (id, name, provider_type) in providers {
        provider_options.push_str(&format!(
            r#"<label class="label cursor-pointer justify-start gap-3 rounded-lg border border-slate-200 bg-white px-3 py-2 hover:border-brand/40">
<input type="checkbox" name="provider_ids[]" value="{}" class="checkbox checkbox-sm checkbox-primary" />
<span class="text-sm font-semibold text-slate-700">{}</span>
<span class="badge badge-ghost text-[10px] uppercase tracking-wider">{}</span>
</label>"#,
            id, name, provider_type
        ));
    }
    if provider_options.is_empty() {
        provider_options.push_str(r#"<div class="text-xs text-slate-400 px-2 py-3">No providers available</div>"#);
    }

    let body = ui::templates::KEYS_PAGE.replace("{{provider_multi_items}}", &provider_options);
    if headers.contains_key("hx-request") {
        Html(body).into_response()
    } else {
        Html(ui::page("UniGateway - API Keys", &body)).into_response()
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

    let api_keys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let providers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let services: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM services")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let content = ui::stats_partial(total, api_keys, providers, services);

    Html(content).into_response()
}

pub(crate) async fn admin_providers_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let providers: Vec<(i64, String, String, Option<String>, Option<String>, i64, Option<String>)> =
        sqlx::query_as(
            "SELECT
                p.id,
                p.name,
                p.provider_type,
                p.endpoint_id,
                p.base_url,
                COUNT(sp.service_id) AS service_count,
                GROUP_CONCAT(sp.service_id, ', ') AS service_ids
             FROM providers p
             LEFT JOIN service_providers sp ON sp.provider_id = p.id
             GROUP BY p.id, p.name, p.provider_type, p.endpoint_id, p.base_url
             ORDER BY p.id DESC",
        )
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut rows_html = String::new();
    for (id, name, ptype, endpoint_id, url, service_count, _) in providers {
        let first_char = name.chars().next().unwrap_or('?');
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                  <div class='w-8 h-8 bg-slate-100 rounded-lg flex items-center justify-center text-slate-400 font-bold group-hover:bg-brand group-hover:text-white transition-all uppercase text-[11px]'>
                      {}
                  </div>
                  <button onclick='openProviderDetail({})' class='font-bold text-slate-700 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1.5 rounded-md text-[10px] uppercase tracking-widest h-auto shadow-none'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <code class='text-[12px] font-mono text-slate-600 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code>
                  <code class='text-[11px] font-mono text-slate-500 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <button onclick='openProviderDetail({})' class='w-fit badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1.5 rounded-md text-[10px] uppercase tracking-widest h-auto shadow-none hover:border-brand/30 hover:text-brand transition-colors'>{} linked</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  hx-delete='/admin/providers/{}'
                  hx-target='#providers-list'
                  hx-confirm='Are you sure you want to remove this provider?'
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  Remove
                </button>
              </td>
            </tr>",
            first_char,
            id,
            name,
            ptype,
            endpoint_id.unwrap_or_default(),
            url.unwrap_or_default(),
            id,
            service_count,
            id
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='5' class='text-center py-20 text-slate-300 font-bold'>No model providers found</td></tr>");
    }

    let final_html = ui::templates::PROVIDERS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_services_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DeleteServiceQuery>,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if service_id == "default" {
        return (StatusCode::BAD_REQUEST, Html("Default service cannot be deleted".to_string())).into_response();
    }

    let token_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE service_id = ?")
        .bind(&service_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    if token_count > 0 && query.force.unwrap_or(0) != 1 {
        return (
            StatusCode::BAD_REQUEST,
            Html(format!("Please delete the {} linked API key(s) first, or use force delete.", token_count)),
        )
            .into_response();
    }

    if token_count > 0 {
        let _ = sqlx::query("DELETE FROM api_key_limits WHERE api_key IN (SELECT key FROM api_keys WHERE service_id = ?)")
            .bind(&service_id)
            .execute(&state.pool)
            .await;

        let _ = sqlx::query("DELETE FROM api_keys WHERE service_id = ?")
            .bind(&service_id)
            .execute(&state.pool)
            .await;
    }

    let _ = sqlx::query("DELETE FROM service_providers WHERE service_id = ?")
        .bind(&service_id)
        .execute(&state.pool)
        .await;

    let _ = sqlx::query("DELETE FROM services WHERE id = ?")
        .bind(&service_id)
        .execute(&state.pool)
        .await;

    admin_services_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_api_keys_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let keys: Vec<(String, Option<String>, String, Option<String>, String)> =
        sqlx::query_as(
            "SELECT k.key, k.name, k.service_id, s.name, k.created_at
             FROM api_keys k
             LEFT JOIN services s ON s.id = k.service_id
             ORDER BY k.created_at DESC",
        )
            .fetch_all(&state.pool)
            .await
            .unwrap_or_default();

    let mut rows_html = String::new();
    for (key, name, service_id, service_name, created_at) in keys {
        let display_name = name.unwrap_or_default();
        let display_service_name = service_name.unwrap_or_else(|| {
            if service_id == "default" {
                "Default Service".to_string()
            } else {
                service_id.clone()
            }
        });
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                    <div class='w-8 h-8 rounded-lg bg-slate-50 border border-slate-100 flex items-center justify-center text-slate-400 font-bold text-xs transition-all group-hover:bg-brand/10 group-hover:border-brand/20 group-hover:text-brand'>
                      {}
                    </div>
                    <div class='flex flex-col'>
                      <button onclick='openApiKeyDetail(&quot;{}&quot;)' class='w-fit text-left font-bold text-slate-800 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                    </div>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                    <code class='text-[13px] font-mono font-bold text-slate-700 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight'>{}</code>
                    <button onclick='copyApiKey(&quot;{}&quot;)' class='btn btn-ghost btn-xs h-8 min-h-0 rounded-lg border border-slate-200 bg-white px-3 font-bold text-slate-600 hover:bg-slate-50'>Copy</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-emerald-50 border-emerald-100 text-emerald-600 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none'>Active</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col'>
                  <button
                    onclick='openServiceDetail(&quot;{}&quot;)'
                    class='w-fit text-[12px] font-bold text-brand hover:text-teal-800 transition-colors tracking-tight'
                  >
                    {}
                  </button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  onclick=\"deleteApiKey('{}')\"
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  Remove
                </button>
              </td>
            </tr>",
            display_name.chars().next().unwrap_or('K').to_ascii_uppercase(),
            key,
            display_name,
            key,
            key,
            service_id,
            display_service_name,
            created_at,
            key
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='6' class='text-center py-20 text-slate-300 font-bold'>No API keys found</td></tr>");
    }

    let final_html = ui::templates::KEYS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_services_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    #[derive(sqlx::FromRow)]
    struct ServiceRow {
        id: String,
        name: String,
        created_at: String,
        provider_count: i64,
        provider_names: Option<String>,
        token_count: i64,
        token_names: Option<String>,
    }

    let services: Vec<ServiceRow> = sqlx::query_as(
        "SELECT
            s.id,
            s.name,
            s.created_at,
            COUNT(DISTINCT sp.provider_id) AS provider_count,
            GROUP_CONCAT(DISTINCT p.name) AS provider_names,
            COUNT(DISTINCT k.key) AS token_count,
            GROUP_CONCAT(DISTINCT COALESCE(k.name, k.key)) AS token_names
         FROM services s
         LEFT JOIN service_providers sp ON sp.service_id = s.id
         LEFT JOIN providers p ON p.id = sp.provider_id
         LEFT JOIN api_keys k ON k.service_id = s.id
         GROUP BY s.id, s.name, s.created_at
         ORDER BY s.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut rows_html = String::new();
    for row in services {
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                  <div class='w-8 h-8 rounded-lg bg-slate-50 border border-slate-100 flex items-center justify-center text-slate-400 font-bold text-xs transition-all group-hover:bg-brand/10 group-hover:border-brand/20 group-hover:text-brand'>
                    {}
                  </div>
                  <button onclick='openServiceDetail(&quot;{}&quot;)' class='w-fit text-left font-bold text-slate-800 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <code class='text-[11px] font-mono font-bold text-slate-600 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight w-fit'>{}</code>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none w-fit'>{} linked</span>
                  <code class='text-[11px] font-mono text-slate-500 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight w-fit'>{}</code>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none w-fit'>{} key(s)</span>
                  <div class='text-[11px] text-slate-500 font-medium leading-5 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 w-fit'>{}</div>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button onclick='deleteService(&quot;{}&quot;, {})' class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'>Remove</button>
              </td>
            </tr>",
            row.name.chars().next().unwrap_or('S').to_ascii_uppercase(),
            row.id,
            row.name,
            row.id,
            row.provider_count,
            row.provider_names.unwrap_or_else(|| "No providers bound".to_string()),
            row.token_count,
            row.token_names.unwrap_or_else(|| "No API keys".to_string()),
            row.created_at,
            row.id,
            row.token_count,
        ));
    }

    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='6' class='text-center py-20 text-slate-300 font-bold'>No services found</td></tr>");
    }

    let final_html = ui::templates::SERVICES_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_logs_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    #[derive(sqlx::FromRow)]
    struct LogRow {
        created_at: String,
        endpoint: String,
        provider: String,
        status_code: i64,
        latency_ms: i64,
    }

    let logs: Vec<LogRow> = sqlx::query_as(
        "SELECT created_at, endpoint, provider, status_code, latency_ms
         FROM request_stats ORDER BY id DESC LIMIT 20",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut rows_html = String::new();
    for row in logs {
        let status_val = row.status_code;
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
                <code class='text-[12px] font-mono font-bold text-slate-700 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md tracking-tight'>{}</code>
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
            row.created_at,
            row.endpoint,
            status_class,
            status_val,
            row.latency_ms,
            row.provider
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='5' class='text-center py-20 text-slate-300 font-bold'>No logs found</td></tr>");
    }

    Html(rows_html).into_response()
}

pub(crate) async fn admin_create_provider_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<CreateProviderReq>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    if !form.name.trim().is_empty() && !form.endpoint_id.trim().is_empty() {
        let provider_id = sqlx::query(
            "INSERT INTO providers(name, provider_type, endpoint_id, base_url, api_key, model_mapping, is_enabled)
             VALUES(?, ?, ?, ?, ?, ?, 1) RETURNING id",
        )
        .bind(form.name.trim())
        .bind(form.provider_type.trim())
        .bind(form.endpoint_id.trim())
        .bind(form.base_url.as_deref().unwrap_or(""))
        .bind(form.api_key.trim())
        .bind(form.model_mapping.as_deref().unwrap_or(""))
        .fetch_one(&state.pool)
        .await
        .map(|row: sqlx::sqlite::SqliteRow| {
            use sqlx::Row;
            row.get::<i64, _>(0)
        });

        if let Ok(pid) = provider_id {
            let _ = sqlx::query("INSERT OR IGNORE INTO services(id, name) VALUES('default', 'Default Service')")
                .execute(&state.pool)
                .await;

            let _ = sqlx::query("INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES('default', ?)")
                .bind(pid)
                .execute(&state.pool)
                .await;
        }
    }

    admin_providers_list_partial(State(state), headers)
        .await
        .into_response()
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
    Query(query): Query<DeleteApiKeyQuery>,
    axum::extract::Path(key): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let service_id: Option<String> = sqlx::query_scalar("SELECT service_id FROM api_keys WHERE key = ?")
        .bind(&key)
        .fetch_optional(&state.pool)
        .await
        .ok()
        .flatten();

    let _ = sqlx::query("DELETE FROM api_key_limits WHERE api_key = ?")
        .bind(&key)
        .execute(&state.pool)
        .await;

    let _ = sqlx::query("DELETE FROM api_keys WHERE key = ?")
        .bind(&key)
        .execute(&state.pool)
        .await;

    if query.delete_service.unwrap_or(0) == 1 {
        if let Some(service_id) = service_id {
            if service_id != "default" {
                let remaining: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE service_id = ?")
                        .bind(&service_id)
                        .fetch_one(&state.pool)
                        .await
                        .unwrap_or(0);
                if remaining == 0 {
                    let _ = sqlx::query("DELETE FROM service_providers WHERE service_id = ?")
                        .bind(&service_id)
                        .execute(&state.pool)
                        .await;
                    let _ = sqlx::query("DELETE FROM services WHERE id = ?")
                        .bind(&service_id)
                        .execute(&state.pool)
                        .await;
                }
            }
        }
    }

    admin_api_keys_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_create_api_key_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawForm(raw_form): RawForm,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut name = String::new();
    let mut provider_ids: Vec<i64> = Vec::new();

    for (key, value) in url::form_urlencoded::parse(raw_form.as_ref()) {
        let key = key.as_ref();
        let value = value.into_owned();
        match key {
            "name" => name = value,
            "provider_ids" | "provider_ids[]" => {
                if let Ok(id) = value.parse::<i64>() {
                    provider_ids.push(id);
                }
            }
            _ => {}
        }
    }

    provider_ids.sort_unstable();
    provider_ids.dedup();

    if name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Html("Token name is required".to_string()),
        )
            .into_response();
    }

    let key = format!("sk-{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    let service_id = if provider_ids.is_empty() {
        "default".to_string()
    } else {
        format!("svc-{}", uuid::Uuid::new_v4().simple())
    };
    let service_name = if provider_ids.is_empty() {
        "Default Service".to_string()
    } else {
        format!("{} Service", name.trim())
    };

    let _ = sqlx::query("INSERT OR IGNORE INTO services(id, name) VALUES('default', 'Default Service')")
        .execute(&state.pool)
        .await;

    if !provider_ids.is_empty() {
        let _ = sqlx::query("INSERT OR IGNORE INTO services(id, name) VALUES(?, ?)")
            .bind(&service_id)
            .bind(&service_name)
            .execute(&state.pool)
            .await;

        for provider_id in &provider_ids {
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)",
            )
            .bind(&service_id)
            .bind(provider_id)
            .execute(&state.pool)
            .await;
        }
    }

    let insert_result = sqlx::query(
        "INSERT INTO api_keys(name, key, service_id, is_active) VALUES(?, ?, ?, 1)",
    )
    .bind(name.trim())
    .bind(&key)
    .bind(&service_id)
    .execute(&state.pool)
    .await;

    if let Err(err) = insert_result {
        return (
            StatusCode::BAD_REQUEST,
            Html(format!("Failed to create token: {}", err)),
        )
            .into_response();
    }

    let trigger = json!({
        "api-key-created": {
            "key": key,
            "service_id": service_id,
            "service_name": service_name,
            "name": name.trim()
        }
    })
    .to_string();

    let mut response = admin_api_keys_list_partial(State(state), headers)
        .await
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&trigger) {
        response.headers_mut().insert("HX-Trigger", value);
    }
    response
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
    endpoint_id: Option<String>,
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
    endpoint_id: String,
    base_url: Option<String>,
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
pub(crate) struct DeleteApiKeyQuery {
    delete_service: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct DeleteServiceQuery {
    force: Option<i64>,
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
        sqlx::query_as(
            "SELECT id, name, provider_type, endpoint_id, base_url FROM providers ORDER BY id DESC",
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
        "INSERT INTO providers(name, provider_type, endpoint_id, base_url, api_key, model_mapping, is_enabled)
         VALUES(?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(&req.name)
    .bind(&req.provider_type)
    .bind(&req.endpoint_id)
    .bind(req.base_url.as_deref().unwrap_or(""))
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
