use anyhow::{Result, anyhow};
use unigateway_core::{ExecutionTarget, GatewayError, ProxyResponsesRequest};
use unigateway_protocol::{
    RuntimeHttpResponse, render_openai_responses_session,
    render_openai_responses_stream_from_completed,
};

use crate::host::{HostContext, HostPoolSource};

use super::targeting::{build_openai_compatible_target, prepare_host_pool};

pub async fn try_openai_responses_via_core(
    host: &HostContext<'_>,
    source: HostPoolSource<'_>,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> Result<Option<RuntimeHttpResponse>> {
    let pool = match prepare_host_pool(host, source).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_openai_responses_via_core(host, pool, hint, request).await
}

async fn execute_openai_responses_via_core(
    host: &HostContext<'_>,
    pool: unigateway_core::ProviderPool,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> Result<Option<RuntimeHttpResponse>> {
    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;

    let response =
        match execute_openai_responses_with_compat(host, target.clone(), request.clone()).await {
            Ok(response) => response,
            Err(error) if should_retry_responses_without_tools(&request) => {
                execute_openai_responses_with_compat(host, target, without_response_tools(request))
                    .await
                    .map_err(|retry_error| anyhow!(retry_error.to_string()))?
            }
            Err(error) => return Err(anyhow!(error.to_string())),
        };

    Ok(Some(response))
}

async fn execute_openai_responses_with_compat(
    host: &HostContext<'_>,
    target: ExecutionTarget,
    request: ProxyResponsesRequest,
) -> Result<RuntimeHttpResponse, GatewayError> {
    if request.stream {
        match host
            .core_engine()
            .proxy_responses(request.clone(), target.clone())
            .await
        {
            Ok(session) => return Ok(render_openai_responses_session(session)),
            Err(stream_error) => {
                let mut fallback_request = request;
                fallback_request.stream = false;

                return host
                    .core_engine()
                    .proxy_responses(fallback_request, target)
                    .await
                    .map(render_openai_responses_stream_from_completed)
                    .map_err(|fallback_error| {
                        if should_preserve_stream_error(&stream_error, &fallback_error) {
                            stream_error
                        } else {
                            fallback_error
                        }
                    });
            }
        }
    }

    host.core_engine()
        .proxy_responses(request, target)
        .await
        .map(render_openai_responses_session)
}

pub(super) fn without_response_tools(request: ProxyResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        tools: None,
        tool_choice: None,
        ..request
    }
}

fn should_retry_responses_without_tools(request: &ProxyResponsesRequest) -> bool {
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
