use std::sync::Arc;

use crate::error::GatewayError;
use crate::request::ProxyChatRequest;
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, ProxySession, RequestKind, StreamKind,
};

use super::super::UniGatewayEngine;
use super::fallback::{EndpointAttemptOutput, RequestExecutionParams};
use super::support::execute_chat_attempt;

impl UniGatewayEngine {
    /// Dispatches a chat completion request to a specific endpoint or pool with fallbacks.
    /// Returns a session representing the lifecycle of the response stream or monolithic text.
    pub async fn proxy_chat(
        &self,
        mut request: ProxyChatRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
        if let Some(hooks) = &self.inner.hooks {
            hooks.on_request(&mut request).await;
        }
        let streaming = request.stream;
        let chunk_forwarder = self.inner.hooks.clone().map(|hooks| {
            Arc::new(move |chunk: ChatResponseChunk| hooks.on_stream_chunk(&chunk))
                as super::support::ChunkForwarder<ChatResponseChunk>
        });

        match self
            .execute_with_fallback(
                request,
                target,
                RequestExecutionParams {
                    kind: RequestKind::Chat,
                    stream_kind: Some(StreamKind::Chat),
                    streaming,
                },
                chunk_forwarder,
                |driver, context, request, timeout| {
                    Box::pin(execute_chat_attempt(driver, context, request, timeout))
                },
            )
            .await?
        {
            EndpointAttemptOutput::Completed(result) => Ok(ProxySession::Completed(*result)),
            EndpointAttemptOutput::Streaming(streaming) => Ok(ProxySession::Streaming(streaming)),
        }
    }
}
