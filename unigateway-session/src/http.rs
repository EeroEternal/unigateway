use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
};
use serde::Deserialize;

use crate::store::{
    DEFAULT_NAMESPACE, Fingerprint, MemorySessionStore, PublishResult, SessionError, SessionKey,
    SessionPrefix,
};

/// HTTP route prefix configuration for session publish/delete endpoints.
#[derive(Debug, Clone)]
pub struct SessionHttpConfig {
    pub path_prefix: String,
    pub namespace: String,
}

impl Default for SessionHttpConfig {
    fn default() -> Self {
        Self {
            path_prefix: "/v1/gateway".to_string(),
            namespace: DEFAULT_NAMESPACE.to_string(),
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
        .with_state((store, config))
}

type SessionHttpState = (Arc<MemorySessionStore>, SessionHttpConfig);

#[derive(Deserialize)]
struct PublishBody {
    epoch: u64,
    messages: Vec<serde_json::Value>,
    #[serde(default)]
    pinned_boundary: Option<u64>,
    #[serde(default)]
    fingerprint: Option<Fingerprint>,
    #[serde(default)]
    message_count: Option<u64>,
}

async fn publish_session(
    State((store, config)): State<SessionHttpState>,
    Path(session_id): Path<String>,
    Json(body): Json<PublishBody>,
) -> Result<StatusCode, StatusCode> {
    let key = SessionKey::new(config.namespace, session_id);
    let prefix = SessionPrefix {
        epoch: body.epoch,
        messages: body.messages,
        pinned_boundary: body.pinned_boundary,
        fingerprint: body.fingerprint,
        message_count: body.message_count,
    };

    match store.publish_key(&key, prefix) {
        Ok(PublishResult::Created)
        | Ok(PublishResult::Replaced)
        | Ok(PublishResult::AlreadyCurrent) => Ok(StatusCode::NO_CONTENT),
        Err(SessionError::StaleEpoch { .. }) | Err(SessionError::EpochConflict { .. }) => {
            Err(StatusCode::CONFLICT)
        }
        Err(SessionError::Expired(_)) => Err(StatusCode::NOT_FOUND),
        Err(
            SessionError::PrefixTooLarge { .. }
            | SessionError::TailTooLarge { .. }
            | SessionError::AssembledTooLarge { .. },
        ) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        Err(SessionError::Unavailable(_)) => Err(StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

async fn delete_session(
    State((store, config)): State<SessionHttpState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let key = SessionKey::new(config.namespace, session_id);
    store.delete_key(&key).map_err(|error| match error {
        SessionError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;
    Ok(StatusCode::NO_CONTENT)
}
