use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::error::GatewayError;
use crate::pool::ProviderKind;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal,
};

pub use crate::endpoint_context::DriverEndpointContext;

/// Repository for looking up `ProviderDriver` implementations by their ID.
pub trait DriverRegistry: Send + Sync + 'static {
    /// Retrieve a driver instance by its unique identifier.
    fn get(&self, driver_id: &str) -> Option<Arc<dyn ProviderDriver>>;
}

/// High-level trait representing a backend integration logic (e.g., standard OpenAI protocol, Anthropic protocol).
///
/// Implementations of this trait translate normalized core requests (like [`ProxyChatRequest`]) into
/// native bytes, send them via HTTP, and then adapt the response chunk streams back into a
/// generalized format.
pub trait ProviderDriver: Send + Sync + 'static {
    /// Unique identifier for this driver type (e.g., "openai-compatible").
    fn driver_id(&self) -> &str;

    /// The base protocol family this driver fulfills.
    fn provider_kind(&self) -> ProviderKind;

    /// Execute a standard chat completion request over the network.
    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>;

    /// Execute a realtime responses/streaming text request.
    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>;

    /// Execute an embeddings generation request.
    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>>;
}
