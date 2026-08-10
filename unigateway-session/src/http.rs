use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
};
use serde::Deserialize;

use crate::store::{MemorySessionStore, SessionPrefix};

/// HTTP route prefix configuration for session publish/delete endpoints.
#[derive(Debug, Clone)]
pub struct SessionHttpConfig {
    pub path_prefix: String,
}

impl Default for SessionHttpConfig {
    fn default() -> Self {
        Self {
            path_prefix: "/v1/gateway".to_string(),
        }
    }
}

/// Builds Axum routes:
/// - `POST {prefix}/sessions/{id}/publish`
/// - `DELETE {prefix}/sessions/{id}`
pub fn session_router(store: Arc<MemorySessionStore>, config: SessionHttpConfig) -> Router {
    Router::new()
        .route(
            &format!("{}/sessions/:session_id/publish", config.path_prefix),
            post(publish_session),
        )
        .route(
            &format!("{}/sessions/:session_id", config.path_prefix),
            delete(delete_session),
        )
        .with_state(store)
}

#[derive(Deserialize)]
struct PublishBody {
    epoch: u64,
    messages: Vec<serde_json::Value>,
    #[serde(default)]
    pinned_boundary: Option<u64>,
}

async fn publish_session(
    State(store): State<Arc<MemorySessionStore>>,
    Path(session_id): Path<String>,
    Json(body): Json<PublishBody>,
) -> Result<StatusCode, StatusCode> {
    store
        .publish(
            &session_id,
            SessionPrefix {
                epoch: body.epoch,
                messages: body.messages,
                pinned_boundary: body.pinned_boundary,
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_session(
    State(store): State<Arc<MemorySessionStore>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    store
        .delete(&session_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
