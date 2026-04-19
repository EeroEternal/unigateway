use std::io;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;
use http::StatusCode;

/// Neutral byte stream type for protocol-rendered HTTP bodies.
pub type ProtocolByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>;

/// Neutral protocol response body used before the product shell builds framework responses.
pub enum ProtocolResponseBody {
    Json(serde_json::Value),
    ServerSentEvents(ProtocolByteStream),
}

/// Neutral protocol-owned HTTP response shape.
pub struct ProtocolHttpResponse {
    status: StatusCode,
    body: ProtocolResponseBody,
}

impl ProtocolHttpResponse {
    /// Build a response with an explicit status code and JSON body.
    pub fn json(status: StatusCode, body: serde_json::Value) -> Self {
        Self {
            status,
            body: ProtocolResponseBody::Json(body),
        }
    }

    /// Build a `200 OK` JSON response.
    pub fn ok_json(body: serde_json::Value) -> Self {
        Self::json(StatusCode::OK, body)
    }

    /// Build a `200 OK` server-sent events response.
    pub fn ok_sse(stream: ProtocolByteStream) -> Self {
        Self {
            status: StatusCode::OK,
            body: ProtocolResponseBody::ServerSentEvents(stream),
        }
    }

    /// Decompose into status and neutral body parts for the product shell.
    pub fn into_parts(self) -> (StatusCode, ProtocolResponseBody) {
        (self.status, self.body)
    }
}
