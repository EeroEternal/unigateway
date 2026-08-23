use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ResponseStream,
    ResponsesEvent, ResponsesFinal, StreamingResponse,
};

use super::super::reporting::{
    SharedStreamState, StreamingAttemptContext, new_shared_stream_state,
    with_streaming_attempt_reports,
};

/// The per-attempt output of an execute closure, unified across request kinds
/// so the fallback skeleton can handle completed and streaming sessions
/// generically. `Chunk` is `()` for completed-only kinds (embeddings).
pub(super) enum EndpointAttemptOutput<Chunk, Final> {
    Completed(Box<CompletedResponse<Final>>),
    Streaming(StreamingResponse<Chunk, Final>),
}

/// Optional per-chunk forwarder used by streaming request kinds that mirror
/// chunks to `GatewayHooks::on_stream_chunk` (chat-only today). Kept as a
/// type-erased callback over owned chunks so the fallback skeleton stays
/// generic over `Chunk`.
pub(super) type ChunkForwarder<Chunk> = Arc<dyn Fn(Chunk) -> BoxFuture<'static, ()> + Send + Sync>;

/// Consolidated streaming-branch handling for the fallback skeleton: shared
/// stream-state bookkeeping, optional per-chunk hook forwarding, and attempt
/// report wiring.
pub(super) async fn observe_stream_outcome<Chunk, Final>(
    mut streaming: StreamingResponse<Chunk, Final>,
    context: StreamingAttemptContext,
    chunk_forwarder: Option<ChunkForwarder<Chunk>>,
) -> StreamingResponse<Chunk, Final>
where
    Chunk: Send + Clone + 'static,
    Final: Send + 'static,
{
    let shared_stream_state = new_shared_stream_state(&context);
    shared_stream_state.started().await;

    if let Some(forwarder) = chunk_forwarder {
        let shared_stream_state = shared_stream_state.clone();
        streaming.stream = observe_stream(
            streaming.stream,
            shared_stream_state.clone(),
            move |chunk| {
                let shared_stream_state = shared_stream_state.clone();
                let forwarder = forwarder.clone();
                let chunk = chunk.clone();
                async move {
                    shared_stream_state.record_chunk().await;
                    forwarder(chunk).await;
                }
            },
        );
    } else {
        let shared_stream_state = shared_stream_state.clone();
        streaming.stream = observe_stream(
            streaming.stream,
            shared_stream_state.clone(),
            move |_chunk| {
                let shared_stream_state = shared_stream_state.clone();
                async move {
                    shared_stream_state.record_chunk().await;
                }
            },
        );
    }

    with_streaming_attempt_reports(streaming, context, shared_stream_state)
}

pub(super) fn observe_stream<Chunk, Hook, HookFuture>(
    mut stream: ResponseStream<Chunk>,
    shared_stream_state: SharedStreamState,
    hook: Hook,
) -> ResponseStream<Chunk>
where
    Chunk: Send + 'static,
    Hook: Fn(&Chunk) -> HookFuture + Send + Sync + 'static,
    HookFuture: std::future::Future<Output = ()> + Send + 'static,
{
    let (sender, receiver) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(item) = stream.next().await {
            if let Ok(ref chunk) = item {
                hook(chunk).await;
            }
            if sender.send(item).is_err() {
                break;
            }
        }
        shared_stream_state.mark_drained();
    });

    Box::pin(UnboundedReceiverStream::new(receiver))
}

pub(super) async fn execute_chat_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyChatRequest,
    timeout: Option<Duration>,
) -> Result<EndpointAttemptOutput<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    let session = if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_chat(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })??
    } else {
        driver.execute_chat(endpoint, request).await?
    };
    Ok(match session {
        crate::response::ProxySession::Completed(result) => {
            EndpointAttemptOutput::Completed(Box::new(result))
        }
        crate::response::ProxySession::Streaming(streaming) => {
            EndpointAttemptOutput::Streaming(streaming)
        }
    })
}

pub(super) async fn execute_responses_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyResponsesRequest,
    timeout: Option<Duration>,
) -> Result<EndpointAttemptOutput<ResponsesEvent, ResponsesFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    let session = if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_responses(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })??
    } else {
        driver.execute_responses(endpoint, request).await?
    };
    Ok(match session {
        crate::response::ProxySession::Completed(result) => {
            EndpointAttemptOutput::Completed(Box::new(result))
        }
        crate::response::ProxySession::Streaming(streaming) => {
            EndpointAttemptOutput::Streaming(streaming)
        }
    })
}

pub(super) async fn execute_embeddings_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyEmbeddingsRequest,
    timeout: Option<Duration>,
) -> Result<EndpointAttemptOutput<(), EmbeddingsResponse>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    let response = if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_embeddings(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })??
    } else {
        driver.execute_embeddings(endpoint, request).await?
    };
    Ok(EndpointAttemptOutput::Completed(Box::new(response)))
}
