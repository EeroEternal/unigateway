use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};

use crate::{
    app::types::AppState,
    ui,
};

use super::{
    queries::{
        find_api_key_detail, find_provider_detail, find_service_detail, list_api_keys_for_service,
        list_provider_options, list_providers_for_service, list_services_by_provider,
    },
    render::{
        render_provider_detail_service_rows, render_provider_options,
        render_service_detail_api_key_rows, render_service_detail_provider_rows,
    },
    shell::{ensure_ui_login, ensure_ui_login_or_redirect, render_hx_or_full, render_hx_or_html},
};

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
    if let Err(response) = ensure_ui_login_or_redirect(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, || ui::templates::ADMIN_PAGE.to_string(), ui::admin_page)
}

pub(crate) async fn admin_provider_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let provider_row = find_provider_detail(&state.pool, id).await;

    let Some(provider_row) = provider_row else {
        return (
            StatusCode::NOT_FOUND,
            Html("Provider not found".to_string()),
        )
            .into_response();
    };

    let bound_services = list_services_by_provider(&state.pool, provider_row.id).await;

    let service_rows = render_provider_detail_service_rows(bound_services);

    let body = ui::templates::PROVIDER_DETAIL_PAGE
        .replace("{{provider_name}}", &provider_row.name)
        .replace("{{provider_type}}", &provider_row.provider_type)
        .replace(
            "{{endpoint_id}}",
            &provider_row.endpoint_id.unwrap_or_else(|| "-".to_string()),
        )
        .replace(
            "{{base_url}}",
            &provider_row.base_url.unwrap_or_else(|| "-".to_string()),
        )
        .replace("{{service_rows}}", &service_rows);

    render_hx_or_full(&headers, body, ui::provider_detail_page)
}

pub(crate) async fn admin_service_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let service_row = find_service_detail(&state.pool, &service_id).await;

    let Some(service_row) = service_row else {
        return (StatusCode::NOT_FOUND, Html("Service not found".to_string())).into_response();
    };

    let service_id = service_row.id;
    let service_name = service_row.name;
    let created_at = service_row.created_at;

    let providers = list_providers_for_service(&state.pool, &service_id).await;

    let api_keys = list_api_keys_for_service(&state.pool, &service_id).await;

    let provider_rows = render_service_detail_provider_rows(providers);

    let api_key_rows = render_service_detail_api_key_rows(api_keys);

    let body = ui::templates::SERVICE_DETAIL_PAGE
        .replace("{{service_name}}", &service_name)
        .replace("{{service_id}}", &service_id)
        .replace("{{created_at}}", &created_at)
        .replace("{{provider_rows}}", &provider_rows)
        .replace("{{api_key_rows}}", &api_key_rows);

    render_hx_or_full(&headers, body, ui::service_detail_page)
}

pub(crate) async fn admin_api_key_detail_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(api_key): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let row = find_api_key_detail(&state.pool, &api_key).await;

    let Some(row) = row else {
        return (StatusCode::NOT_FOUND, Html("API key not found".to_string())).into_response();
    };

    let service_name = row.service_name.unwrap_or_else(|| {
        if row.service_id == "default" {
            "Default Service".to_string()
        } else {
            row.service_id.clone()
        }
    });

    let body = ui::templates::API_KEY_DETAIL_PAGE
        .replace("{{api_key_name}}", &row.name.unwrap_or_default())
        .replace("{{api_key_value}}", &row.key)
        .replace("{{created_at}}", &row.created_at)
        .replace("{{service_id}}", &row.service_id)
        .replace("{{service_name}}", &service_name);

    render_hx_or_full(&headers, body, ui::api_key_detail_page)
}

pub(crate) async fn admin_services_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, || ui::templates::SERVICES_PAGE.to_string(), ui::services_page)
}

pub(crate) async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, || ui::templates::ADMIN_PAGE.to_string(), ui::admin_page)
}

pub(crate) async fn admin_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, ui::render_providers_body, ui::providers_page)
}

pub(crate) async fn admin_api_keys_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let providers = list_provider_options(&state.pool).await;

    let provider_options = render_provider_options(providers);

    let body = ui::templates::KEYS_PAGE.replace("{{provider_multi_items}}", &provider_options);
    render_hx_or_full(&headers, body, |body| ui::page("UniGateway - API Keys", body))
}

pub(crate) async fn admin_logs_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, || ui::templates::LOGS_PAGE.to_string(), ui::logs_page)
}

pub(crate) async fn admin_settings_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    render_hx_or_html(&headers, || ui::templates::SETTINGS_PAGE.to_string(), ui::settings_page)
}
