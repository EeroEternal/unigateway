use std::sync::Arc;

mod legacy_runtime;
mod support;

use axum::{
    extract::{Json, State},
    http::HeaderMap,
    response::Response,
};

use crate::types::AppState;

use self::support::{
    handle_anthropic_messages_request, handle_openai_chat_request,
    handle_openai_embeddings_request, handle_openai_responses_request,
};

// ---------------------------------------------------------------------------
// OpenAI Chat
// ---------------------------------------------------------------------------

pub(crate) async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_chat_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// OpenAI Responses
// ---------------------------------------------------------------------------

pub(crate) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_responses_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// Anthropic Messages
// ---------------------------------------------------------------------------

pub(crate) async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_anthropic_messages_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

pub(crate) async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_embeddings_request(&state, &headers, &payload).await
}
