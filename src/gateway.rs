use std::sync::Arc;

mod support;

use axum::{
    Router,
    extract::{Json, State},
    http::HeaderMap,
    response::Response,
    routing::post,
};

use crate::types::GatewayRequestState;

use self::support::{
    handle_anthropic_messages_request, handle_openai_chat_request,
    handle_openai_embeddings_request, handle_openai_responses_request,
};

pub(crate) fn router() -> Router<Arc<GatewayRequestState>> {
    Router::new()
        .route("/v1/responses", post(openai_responses))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/embeddings", post(openai_embeddings))
        .route("/v1/messages", post(anthropic_messages))
}

// ---------------------------------------------------------------------------
// OpenAI Chat
// ---------------------------------------------------------------------------

pub(crate) async fn openai_chat(
    State(state): State<Arc<GatewayRequestState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_chat_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// OpenAI Responses
// ---------------------------------------------------------------------------

pub(crate) async fn openai_responses(
    State(state): State<Arc<GatewayRequestState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_responses_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// Anthropic Messages
// ---------------------------------------------------------------------------

pub(crate) async fn anthropic_messages(
    State(state): State<Arc<GatewayRequestState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_anthropic_messages_request(&state, &headers, &payload).await
}

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

pub(crate) async fn openai_embeddings(
    State(state): State<Arc<GatewayRequestState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    handle_openai_embeddings_request(&state, &headers, &payload).await
}
