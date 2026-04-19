use std::io;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;
use http::StatusCode;

/// Neutral byte stream type for protocol-rendered HTTP bodies.
pub type RuntimeByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>;

/// Neutral protocol response body used before the product shell builds framework responses.
pub enum RuntimeResponseBody {
    Json(serde_json::Value),
    ServerSentEvents(RuntimeByteStream),
}

/// Neutral protocol-owned HTTP response shape.
///
/// The `Runtime*` prefix is a compatibility holdover from the pre-split runtime crate.
/// The type now belongs to `unigateway-protocol` and is intentionally framework-agnostic;
/// only the root product shell should convert it into `axum::Response`.
pub struct RuntimeHttpResponse {
    status: StatusCode,
    body: RuntimeResponseBody,
}

impl RuntimeHttpResponse {
    /// Build a response with an explicit status code and JSON body.
    pub fn json(status: StatusCode, body: serde_json::Value) -> Self {
        Self {
            status,
            body: RuntimeResponseBody::Json(body),
        }
    }

    /// Build a `200 OK` JSON response.
    pub fn ok_json(body: serde_json::Value) -> Self {
        Self::json(StatusCode::OK, body)
    }

    /// Build a `200 OK` server-sent events response.
    pub fn ok_sse(stream: RuntimeByteStream) -> Self {
        Self {
            status: StatusCode::OK,
            body: RuntimeResponseBody::ServerSentEvents(stream),
        }
    }

    /// Decompose into status and neutral body parts for the product shell.
    pub fn into_parts(self) -> (StatusCode, RuntimeResponseBody) {
        (self.status, self.body)
    }
}
