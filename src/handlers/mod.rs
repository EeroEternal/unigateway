pub mod admin;
pub mod auth;
pub mod chat;
pub mod health;

use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
};
use std::sync::Arc;
use crate::server::AppState;

pub async fn home() -> impl IntoResponse {
    Redirect::to("/admin")
}
