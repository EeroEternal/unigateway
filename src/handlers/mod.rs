pub mod admin;
pub mod auth;
pub mod chat;
pub mod health;

use axum::response::{IntoResponse, Redirect};


pub async fn home() -> impl IntoResponse {
    Redirect::to("/admin")
}
