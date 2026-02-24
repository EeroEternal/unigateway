use std::sync::Arc;

use axum::{
    extract::{Form, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use rand::{distributions::Alphanumeric, Rng};
use sqlx::SqlitePool;

use crate::ui;

use super::{storage::hash_password, types::{AppState, LoginForm}};

pub(crate) async fn login_page() -> impl IntoResponse {
    Html(ui::login_page())
}

pub(crate) async fn login(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let user = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, password_hash FROM users WHERE username = ?",
    )
    .bind(&form.username)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((user_id, password_hash))) = user else {
        return Html(ui::login_error_page()).into_response();
    };

    if hash_password(&form.password) != password_hash {
        return Html(ui::login_error_page()).into_response();
    }

    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();

    if sqlx::query("INSERT INTO sessions(token, user_id) VALUES(?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session create failed").into_response();
    }

    let mut headers = HeaderMap::new();
    if let Ok(cookie) =
        format!("unigateway_session={token}; Path=/; HttpOnly; SameSite=Lax").parse()
    {
        headers.insert(header::SET_COOKIE, cookie);
    }

    (headers, Redirect::to("/admin")).into_response()
}

pub(crate) async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(token) = get_cookie_token(&headers) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.pool)
            .await;
    }

    let mut response = Redirect::to("/login").into_response();
    if let Ok(cookie) = "unigateway_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".parse() {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

pub(crate) async fn ensure_login(pool: &SqlitePool, headers: &HeaderMap) -> bool {
    let Some(token) = get_cookie_token(headers) else {
        return false;
    };

    match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE token = ?")
        .bind(token)
        .fetch_one(pool)
        .await
    {
        Ok(count) => count > 0,
        Err(_) => false,
    }
}

fn get_cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|part| {
                let item = part.trim();
                item.strip_prefix("unigateway_session=")
                    .map(|v| v.to_string())
            })
        })
}
