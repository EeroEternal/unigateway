use std::sync::Arc;

use axum::{
    Form,
    extract::{Query, RawForm, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse},
};
use serde_json::json;

use crate::{
    app::types::AppState,
    ui,
};

use super::{
    dto::{CreateProviderReq, DeleteApiKeyQuery, DeleteServiceQuery},
    mutations::{
        create_api_key_with_service, create_provider_and_bind_default, delete_api_key_and_maybe_service,
        delete_provider, delete_service,
    },
    queries::{
        count_api_keys_by_service, fetch_dashboard_stats, find_service_id_for_api_key,
        list_api_key_rows, list_log_rows, list_provider_rows, list_service_rows,
    },
    render::{
        render_api_key_list_rows, render_log_rows, render_provider_list_rows,
        render_service_list_rows,
    },
    shell::ensure_ui_login,
};

pub(crate) async fn admin_stats_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let stats = fetch_dashboard_stats(&state.pool).await;

    let content = ui::stats_partial(stats.total, stats.api_keys, stats.providers, stats.services);

    Html(content).into_response()
}

pub(crate) async fn admin_providers_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let providers = list_provider_rows(&state.pool).await;

    let rows_html = render_provider_list_rows(providers);

    let final_html = ui::templates::PROVIDERS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_services_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DeleteServiceQuery>,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    if service_id == "default" {
        return (
            StatusCode::BAD_REQUEST,
            Html("Default service cannot be deleted".to_string()),
        )
            .into_response();
    }

    let token_count = count_api_keys_by_service(&state.pool, &service_id).await;

    if token_count > 0 && query.force.unwrap_or(0) != 1 {
        return (
            StatusCode::BAD_REQUEST,
            Html(format!(
                "Please delete the {} linked API key(s) first, or use force delete.",
                token_count
            )),
        )
            .into_response();
    }

    delete_service(&state.pool, &service_id, token_count).await;

    admin_services_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_api_keys_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let keys = list_api_key_rows(&state.pool).await;

    let rows_html = render_api_key_list_rows(keys);

    let final_html = ui::templates::KEYS_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_services_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let services = list_service_rows(&state.pool).await;

    let rows_html = render_service_list_rows(services);

    let final_html = ui::templates::SERVICES_LIST_PARTIAL.replace("{{rows}}", &rows_html);
    Html(final_html).into_response()
}

pub(crate) async fn admin_logs_list_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let logs = list_log_rows(&state.pool).await;

    let rows_html = render_log_rows(logs);

    Html(rows_html).into_response()
}

pub(crate) async fn admin_create_provider_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(form): Form<CreateProviderReq>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    create_provider_and_bind_default(
        &state.pool,
        &form.name,
        &form.provider_type,
        &form.endpoint_id,
        form.base_url.as_deref(),
        &form.api_key,
        form.model_mapping.as_deref(),
    )
    .await;

    admin_providers_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_providers_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    delete_provider(&state.pool, id).await;

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
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
    }

    let service_id = find_service_id_for_api_key(&state.pool, &key).await;

    delete_api_key_and_maybe_service(
        &state.pool,
        &key,
        service_id,
        query.delete_service.unwrap_or(0) == 1,
    )
    .await;

    admin_api_keys_list_partial(State(state), headers)
        .await
        .into_response()
}

pub(crate) async fn admin_create_api_key_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawForm(raw_form): RawForm,
) -> impl IntoResponse {
    if let Err(response) = ensure_ui_login(&state, &headers).await {
        return response;
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

    let insert_result = create_api_key_with_service(
        &state.pool,
        &name,
        &provider_ids,
        &key,
        &service_id,
        &service_name,
    )
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
