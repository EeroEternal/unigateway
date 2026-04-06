use std::collections::HashMap;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::error::GatewayError;
use crate::pool::{EndpointId, ModelPolicy, ProviderKind, SecretString};
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal,
};

pub trait DriverRegistry: Send + Sync + 'static {
    fn get(&self, driver_id: &str) -> Option<Arc<dyn ProviderDriver>>;
}

pub trait ProviderDriver: Send + Sync + 'static {
    fn driver_id(&self) -> &str;

    fn provider_kind(&self) -> ProviderKind;

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>;

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>;

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEndpointContext {
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub base_url: String,
    pub api_key: SecretString,
    pub model_policy: ModelPolicy,
    pub metadata: HashMap<String, String>,
}
