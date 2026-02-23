use std::sync::Arc;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    Form,
};
use crate::server::AppState;
use crate::handlers::auth::ensure_login;
use crate::db::models::{CreateProviderForm, Provider};
use askama::Template;

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    title: String,
}

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate {
    total: i64,
    openai_count: i64,
    anthropic_count: i64,
}

#[derive(Template)]
#[template(path = "providers.html")]
struct ProvidersTemplate {
    title: String,
}

#[derive(Template)]
#[template(path = "providers_list.html")]
struct ProvidersListTemplate {
    providers: Vec<Provider>,
}

pub async fn admin_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return Redirect::to("/login").into_response();
    }

    let template = DashboardTemplate {
        title: "UniGateway Admin".to_string(),
    };
    Html(template.render().unwrap()).into_response()
}

pub async fn admin_stats_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
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

    let template = StatsTemplate {
        total,
        openai_count,
        anthropic_count,
    };

    Html(template.render().unwrap()).into_response()
}

// --- Providers Management ---

pub async fn providers_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return Redirect::to("/login").into_response();
    }

    let template = ProvidersTemplate {
        title: "Providers - UniGateway".to_string(),
    };
    Html(template.render().unwrap()).into_response()
}

pub async fn providers_list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let providers = sqlx::query_as::<_, Provider>("SELECT * FROM providers ORDER BY id DESC")
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();

    let template = ProvidersListTemplate { providers };
    Html(template.render().unwrap()).into_response()
}

pub async fn create_provider(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CreateProviderForm>,
) -> impl IntoResponse {
    let _ = sqlx::query(
        "INSERT INTO providers (name, provider_type, base_url, api_key) VALUES (?, ?, ?, ?)",
    )
    .bind(form.name)
    .bind(form.provider_type)
    .bind(form.base_url)
    .bind(form.api_key)
    .execute(&state.pool)
    .await;

    providers_list(State(state)).await
}

pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let _ = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await;

    providers_list(State(state)).await
}
