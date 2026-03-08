use std::sync::Arc;

use axum::{
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::app::{auth::ensure_login, types::AppState};

pub(crate) async fn ensure_ui_login(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(), Response> {
    if ensure_login(&state.pool, headers).await {
        Ok(())
    } else {
        Err(axum::http::StatusCode::UNAUTHORIZED.into_response())
    }
}

pub(crate) async fn ensure_ui_login_or_redirect(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<(), Response> {
    if ensure_login(&state.pool, headers).await {
        Ok(())
    } else {
        Err(Redirect::to("/login").into_response())
    }
}

pub(crate) fn render_hx_or_full(
    headers: &HeaderMap,
    body: String,
    full: impl FnOnce(&str) -> String,
) -> Response {
    if headers.contains_key("hx-request") {
        Html(body).into_response()
    } else {
        Html(full(&body)).into_response()
    }
}

pub(crate) fn render_hx_or_html(
    headers: &HeaderMap,
    partial: impl FnOnce() -> String,
    full: impl FnOnce() -> String,
) -> Response {
    if headers.contains_key("hx-request") {
        Html(partial()).into_response()
    } else {
        Html(full()).into_response()
    }
}
