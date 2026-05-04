use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ResponseStream,
    ResponsesEvent, ResponsesFinal,
};

use super::super::reporting::SharedStreamState;

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
) -> Result<crate::response::ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_chat(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_chat(endpoint, request).await
    }
}

pub(super) async fn execute_responses_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyResponsesRequest,
    timeout: Option<Duration>,
) -> Result<crate::response::ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_responses(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_responses(endpoint, request).await
    }
}

pub(super) async fn execute_embeddings_attempt(
    driver: Arc<dyn ProviderDriver>,
    endpoint: DriverEndpointContext,
    request: ProxyEmbeddingsRequest,
    timeout: Option<Duration>,
) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
    let endpoint_id = endpoint.endpoint_id.clone();
    if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, driver.execute_embeddings(endpoint, request))
            .await
            .map_err(|_| GatewayError::Transport {
                message: "attempt timed out".to_string(),
                endpoint_id: Some(endpoint_id),
            })?
    } else {
        driver.execute_embeddings(endpoint, request).await
    }
}
