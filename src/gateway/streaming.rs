use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{ChatRequest, StreamChunk, StreamFormat};

use crate::{
    protocol::{UpstreamProtocol, invoke_with_connector_stream},
    routing::ResolvedProvider,
};

pub(super) async fn try_chat_stream(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &ChatRequest,
) -> Result<Response, anyhow::Error> {
    try_chat_stream_raw(
        protocol,
        &provider.base_url,
        &provider.api_key,
        request,
        provider.family_id.as_deref(),
    )
    .await
}

pub(super) async fn try_chat_stream_raw(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    family_id: Option<&str>,
) -> Result<Response, anyhow::Error> {
    let stream =
        invoke_with_connector_stream(protocol, base_url, api_key, request, family_id).await?;
    type BoxErr = Box<dyn std::error::Error + Send + Sync>;
    let sse_stream = stream.map(|r: Result<_, llm_connector::error::LlmConnectorError>| {
        r.map_err(|e| -> BoxErr { Box::new(std::io::Error::other(e.to_string())) })
            .and_then(|resp| {
                StreamChunk::from_openai(&resp, StreamFormat::SSE)
                    .map(|c| Bytes::from(c.to_sse()))
                    .map_err(|e: serde_json::Error| -> BoxErr { Box::new(e) })
            })
    });
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(sse_stream),
    )
        .into_response())
}
