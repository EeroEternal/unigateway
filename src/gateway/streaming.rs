use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use llm_connector::types::{
    ChatRequest, StreamChunk, StreamFormat, streaming::AnthropicSseAdapter,
};

use crate::{
    protocol::{UpstreamProtocol, invoke_with_connector_stream},
    routing::ResolvedProvider,
};

pub(super) async fn try_chat_stream(
    protocol: UpstreamProtocol,
    provider: &ResolvedProvider,
    request: &ChatRequest,
    is_anthropic_downstream: bool,
) -> Result<Response, anyhow::Error> {
    try_chat_stream_raw(
        protocol,
        &provider.base_url,
        &provider.api_key,
        request,
        provider.family_id.as_deref(),
        is_anthropic_downstream,
    )
    .await
}

pub(super) async fn try_chat_stream_raw(
    protocol: UpstreamProtocol,
    base_url: &str,
    api_key: &str,
    request: &ChatRequest,
    family_id: Option<&str>,
    is_anthropic_downstream: bool,
) -> Result<Response, anyhow::Error> {
    let stream =
        invoke_with_connector_stream(protocol, base_url, api_key, request, family_id).await?;
    type BoxErr = Box<dyn std::error::Error + Send + Sync>;

    if is_anthropic_downstream {
        // Anthropic client expects SSE events (message_start, content_block_delta, etc.)
        // We use AnthropicSseAdapter to convert the stream of StreamingResponse chunks.
        // Note: We are using a Mutex to share the adapter state across the stream.
        let adapter = std::sync::Arc::new(std::sync::Mutex::new(AnthropicSseAdapter::new()));

        let sse_stream = stream.flat_map(
            move |r: Result<_, llm_connector::error::LlmConnectorError>| {
                let adapter = adapter.clone();
                // We need to return a stream of bytes.
                // Since flat_map expects a stream/iterator, we can return futures::stream::iter

                let result: Vec<Result<Bytes, BoxErr>> = match r {
                    Ok(resp) => {
                        let mut guard = adapter.lock().unwrap();
                        let events = guard.convert(&resp);
                        events.into_iter().map(|s| Ok(Bytes::from(s))).collect()
                    }
                    Err(e) => {
                        // Log the error for debugging
                        tracing::error!("llm-connector chat_stream failed: {}", e);
                        vec![Err(Box::new(std::io::Error::other(format!(
                            "llm-connector chat_stream failed: {}",
                            e
                        ))) as BoxErr)]
                    }
                };

                futures_util::stream::iter(result)
            },
        );

        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/event-stream")],
            Body::from_stream(sse_stream),
        )
            .into_response())
    } else {
        // Default behavior (OpenAI compatible)
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
}
