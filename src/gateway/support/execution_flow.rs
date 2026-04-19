use std::sync::Arc;

use axum::response::Response;
use tracing::info;
use unigateway_host::{
    core::{HostDispatchOutcome, HostDispatchTarget, HostProtocol, HostRequest, dispatch_request},
    env::{EnvPoolHost, EnvProvider},
    error::HostError,
    host::PoolLookupOutcome,
};

use crate::types::GatewayRequestState;

use super::request_flow::PreparedGatewayRequest;
use super::response_flow::{
    missing_upstream_api_key_response, resolve_core_only_host_flow, respond_prepared_host_result,
};

type BeforeExecuteHook = for<'a> fn(&GatewayRequestState, &PreparedGatewayRequest<'a>);

#[derive(Clone, Copy)]
pub(super) struct HostExecutionSpec {
    endpoint: &'static str,
    protocol: HostProtocol,
    env_provider: EnvProvider,
    unavailable_message: &'static str,
    before_execute: Option<BeforeExecuteHook>,
}

pub(super) fn openai_chat_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/chat/completions",
        protocol: HostProtocol::OpenAiChat,
        env_provider: EnvProvider::OpenAi,
        unavailable_message: "no provider pool available for chat",
        before_execute: None,
    }
}

pub(super) fn openai_responses_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/responses",
        protocol: HostProtocol::OpenAiResponses,
        env_provider: EnvProvider::OpenAi,
        unavailable_message: "no openai-compatible provider available for responses",
        before_execute: None,
    }
}

pub(super) fn anthropic_chat_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/messages",
        protocol: HostProtocol::AnthropicMessages,
        env_provider: EnvProvider::Anthropic,
        unavailable_message: "no provider pool available for messages",
        before_execute: Some(log_anthropic_request_execution),
    }
}

pub(super) fn openai_embeddings_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/embeddings",
        protocol: HostProtocol::OpenAiEmbeddings,
        env_provider: EnvProvider::OpenAi,
        unavailable_message: "no openai-compatible provider available for embeddings",
        before_execute: None,
    }
}

pub(super) async fn execute_prepared_host_request(
    state: &Arc<GatewayRequestState>,
    prepared: &PreparedGatewayRequest<'_>,
    request: HostRequest,
    spec: HostExecutionSpec,
) -> Response {
    if let Some(before_execute) = spec.before_execute {
        before_execute(state.as_ref(), prepared);
    }

    let target = match dispatch_target(state, prepared, spec.env_provider).await {
        Ok(target) => target,
        Err(ResolveDispatchTargetError::MissingUpstreamApiKey) => {
            return respond_prepared_host_result(
                state,
                prepared,
                spec.endpoint,
                Err(missing_upstream_api_key_response()),
            )
            .await;
        }
        Err(ResolveDispatchTargetError::Other(error)) => {
            let result =
                resolve_core_only_host_flow(async move { Err(error) }, spec.unavailable_message)
                    .await;
            return respond_prepared_host_result(state, prepared, spec.endpoint, result).await;
        }
    };

    let result = resolve_core_only_host_flow(
        async move {
            match target {
                DispatchTargetResolution::DispatchTarget(target) => {
                    dispatch_request(
                        &prepared.host,
                        target,
                        spec.protocol,
                        prepared.hint.as_deref(),
                        request,
                    )
                    .await
                }
                DispatchTargetResolution::PoolNotFound => Ok(HostDispatchOutcome::PoolNotFound),
            }
        },
        spec.unavailable_message,
    )
    .await;
    respond_prepared_host_result(state, prepared, spec.endpoint, result).await
}

async fn dispatch_target<'prepared>(
    state: &GatewayRequestState,
    prepared: &'prepared PreparedGatewayRequest<'_>,
    env_provider: EnvProvider,
) -> Result<DispatchTargetResolution<'prepared>, ResolveDispatchTargetError> {
    if let Some(auth) = prepared.auth.as_ref() {
        return Ok(DispatchTargetResolution::DispatchTarget(
            HostDispatchTarget::Service(&auth.key.service_id),
        ));
    }

    let api_key_override = (!prepared.token.is_empty()).then_some(prepared.token.as_str());
    if api_key_override.is_none() && env_api_key(state, env_provider).is_empty() {
        return Err(ResolveDispatchTargetError::MissingUpstreamApiKey);
    }

    state
        .env_pool(env_provider, api_key_override)
        .await
        .map(|pool| match pool {
            PoolLookupOutcome::Found(pool) => {
                DispatchTargetResolution::DispatchTarget(HostDispatchTarget::Pool(pool))
            }
            PoolLookupOutcome::NotFound => DispatchTargetResolution::PoolNotFound,
            _ => DispatchTargetResolution::PoolNotFound,
        })
        .map_err(|error| ResolveDispatchTargetError::Other(HostError::pool_lookup(error)))
}

fn env_api_key(state: &GatewayRequestState, provider: EnvProvider) -> &str {
    state.provider_api_key(provider)
}

fn log_anthropic_request_execution(
    state: &GatewayRequestState,
    prepared: &PreparedGatewayRequest<'_>,
) {
    let endpoint = "/v1/messages";
    let env_api_key = env_api_key(state, EnvProvider::Anthropic);

    info!(
        endpoint,
        gateway_key_matched = prepared.auth.is_some(),
        token_present = !prepared.token.is_empty(),
        "anthropic request authentication result"
    );

    if prepared.auth.is_none() {
        info!(
            endpoint,
            token_present = !prepared.token.is_empty(),
            env_key_present = !env_api_key.is_empty(),
            using_env_fallback = prepared.token.is_empty() && !env_api_key.is_empty(),
            "anthropic request falling back to env upstream key"
        );
    }
}

enum ResolveDispatchTargetError {
    MissingUpstreamApiKey,
    Other(HostError),
}

enum DispatchTargetResolution<'a> {
    DispatchTarget(HostDispatchTarget<'a>),
    PoolNotFound,
}
