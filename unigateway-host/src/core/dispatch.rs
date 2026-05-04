use unigateway_core::{
    GatewayError, ProviderPool, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
use unigateway_protocol::{ProtocolHttpResponse, anthropic_requested_model_alias_or};

use super::chat::{execute_anthropic_chat_via_core, execute_openai_chat_via_core};
use super::embeddings::execute_openai_embeddings_via_core;
use super::responses::execute_openai_responses_via_core;
use crate::error::{HostError, HostResult};
use crate::host::{HostContext, PoolLookupOutcome};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostProtocol {
    OpenAiChat,
    AnthropicMessages,
    OpenAiResponses,
    OpenAiEmbeddings,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum HostRequest {
    Chat(ProxyChatRequest),
    Responses(ProxyResponsesRequest),
    Embeddings(ProxyEmbeddingsRequest),
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum HostDispatchTarget<'a> {
    Service(&'a str),
    Pool(ProviderPool),
    PoolRef(&'a ProviderPool),
}

#[non_exhaustive]
pub enum HostDispatchOutcome {
    Response(ProtocolHttpResponse),
    PoolNotFound,
}

enum ResolvedPool<'a> {
    Owned(ProviderPool),
    Borrowed(&'a ProviderPool),
}

pub async fn dispatch_request(
    host: &HostContext<'_>,
    target: HostDispatchTarget<'_>,
    protocol: HostProtocol,
    hint: Option<&str>,
    request: HostRequest,
) -> HostResult<HostDispatchOutcome> {
    let resolved_pool = match target {
        HostDispatchTarget::Service(service_id) => match host
            .pool_for_service(service_id)
            .await
            .map_err(HostError::pool_lookup)?
        {
            PoolLookupOutcome::Found(pool) => ResolvedPool::Owned(pool),
            PoolLookupOutcome::NotFound => return Ok(HostDispatchOutcome::PoolNotFound),
        },
        HostDispatchTarget::Pool(pool) => ResolvedPool::Owned(pool),
        HostDispatchTarget::PoolRef(pool) => ResolvedPool::Borrowed(pool),
    };

    let pool = match &resolved_pool {
        ResolvedPool::Owned(pool) => pool,
        ResolvedPool::Borrowed(pool) => pool,
    };

    let request_kind = request.kind_name();

    let response = match (protocol, request) {
        (HostProtocol::OpenAiChat, HostRequest::Chat(request)) => {
            execute_openai_chat_via_core(host, pool, hint, request).await?
        }
        (HostProtocol::AnthropicMessages, HostRequest::Chat(request)) => {
            execute_anthropic_chat_via_core(host, pool, hint, request).await?
        }
        (HostProtocol::OpenAiResponses, HostRequest::Responses(request)) => {
            execute_openai_responses_via_core(host, pool, hint, request).await?
        }
        (HostProtocol::OpenAiEmbeddings, HostRequest::Embeddings(request)) => {
            execute_openai_embeddings_via_core(host, pool, hint, request).await?
        }
        (protocol, _) => {
            return Err(HostError::invalid_dispatch_request(
                protocol.as_str(),
                request_kind,
            ));
        }
    };

    Ok(HostDispatchOutcome::Response(response))
}

impl HostRequest {
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Chat(_) => "chat-request",
            Self::Responses(_) => "responses-request",
            Self::Embeddings(_) => "embeddings-request",
        }
    }
}

impl HostProtocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiChat => "openai-chat",
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenAiResponses => "openai-responses",
            Self::OpenAiEmbeddings => "openai-embeddings",
        }
    }
}

pub fn anthropic_requested_model_alias(request: &ProxyChatRequest) -> String {
    anthropic_requested_model_alias_or(&request.metadata, &request.model)
}

pub(super) fn without_response_tools(request: ProxyResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        tools: None,
        tool_choice: None,
        ..request
    }
}

pub(super) fn should_retry_responses_without_tools(request: &ProxyResponsesRequest) -> bool {
    request.tools.is_some() || request.tool_choice.is_some()
}

pub(super) fn should_preserve_stream_error(
    stream_error: &GatewayError,
    fallback_error: &GatewayError,
) -> bool {
    matches!(
        stream_error.terminal_error(),
        GatewayError::InvalidRequest(_)
            | GatewayError::PoolNotFound(_)
            | GatewayError::EndpointNotFound(_)
    ) || matches!(
        fallback_error.terminal_error(),
        GatewayError::InvalidRequest(_)
            | GatewayError::PoolNotFound(_)
            | GatewayError::EndpointNotFound(_)
    )
}
