use std::future::Future;
use std::sync::Arc;

use axum::response::Response;
use tracing::info;
use unigateway_host::{
    flow::{missing_upstream_api_key_response, resolve_core_only_host_flow},
    host::{HostEnvProvider, HostPoolSource},
};
use unigateway_protocol::RuntimeHttpResponse;

use crate::types::GatewayRequestState;

use super::request_flow::PreparedGatewayRequest;
use super::response_flow::respond_prepared_host_result;

type BeforeExecuteHook = for<'a> fn(&GatewayRequestState, &PreparedGatewayRequest<'a>);

#[derive(Clone, Copy)]
pub(super) struct HostExecutionSpec {
    endpoint: &'static str,
    env_provider: HostEnvProvider,
    unavailable_message: &'static str,
    before_execute: Option<BeforeExecuteHook>,
}

pub(super) fn openai_chat_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/chat/completions",
        env_provider: HostEnvProvider::OpenAi,
        unavailable_message: "no provider pool available for chat",
        before_execute: None,
    }
}

pub(super) fn openai_responses_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/responses",
        env_provider: HostEnvProvider::OpenAi,
        unavailable_message: "no openai-compatible provider available for responses",
        before_execute: None,
    }
}

pub(super) fn anthropic_chat_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/messages",
        env_provider: HostEnvProvider::Anthropic,
        unavailable_message: "no provider pool available for messages",
        before_execute: Some(log_anthropic_request_execution),
    }
}

pub(super) fn openai_embeddings_spec() -> HostExecutionSpec {
    HostExecutionSpec {
        endpoint: "/v1/embeddings",
        env_provider: HostEnvProvider::OpenAi,
        unavailable_message: "no openai-compatible provider available for embeddings",
        before_execute: None,
    }
}

pub(super) async fn execute_prepared_host_request<'prepared, Request, Execute, CoreFuture>(
    state: &Arc<GatewayRequestState>,
    prepared: &'prepared PreparedGatewayRequest<'_>,
    request: Request,
    spec: HostExecutionSpec,
    execute: Execute,
) -> Response
where
    Execute: FnOnce(
        &'prepared PreparedGatewayRequest<'_>,
        HostPoolSource<'prepared>,
        Request,
    ) -> CoreFuture,
    CoreFuture: Future<Output = anyhow::Result<Option<RuntimeHttpResponse>>>,
{
    if let Some(before_execute) = spec.before_execute {
        before_execute(state.as_ref(), prepared);
    }

    let source = match host_pool_source(state, prepared, spec.env_provider) {
        Ok(source) => source,
        Err(HostPoolSourceError::MissingUpstreamApiKey) => {
            return respond_prepared_host_result(
                state,
                prepared,
                spec.endpoint,
                Err(missing_upstream_api_key_response()),
            )
            .await;
        }
    };

    let result =
        resolve_core_only_host_flow(execute(prepared, source, request), spec.unavailable_message)
            .await;
    respond_prepared_host_result(state, prepared, spec.endpoint, result).await
}

fn host_pool_source<'prepared>(
    state: &GatewayRequestState,
    prepared: &'prepared PreparedGatewayRequest<'_>,
    env_provider: HostEnvProvider,
) -> Result<HostPoolSource<'prepared>, HostPoolSourceError> {
    if let Some(auth) = prepared.auth.as_ref() {
        return Ok(HostPoolSource::Service(&auth.key.service_id));
    }

    let api_key_override = (!prepared.token.is_empty()).then_some(prepared.token.as_str());
    if api_key_override.is_none() && env_api_key(state, env_provider).is_empty() {
        return Err(HostPoolSourceError::MissingUpstreamApiKey);
    }

    Ok(HostPoolSource::Env {
        provider: env_provider,
        api_key_override,
    })
}

fn env_api_key(state: &GatewayRequestState, provider: HostEnvProvider) -> &str {
    state.provider_api_key(provider)
}

fn log_anthropic_request_execution(
    state: &GatewayRequestState,
    prepared: &PreparedGatewayRequest<'_>,
) {
    let endpoint = "/v1/messages";
    let env_api_key = env_api_key(state, HostEnvProvider::Anthropic);

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

enum HostPoolSourceError {
    MissingUpstreamApiKey,
}
