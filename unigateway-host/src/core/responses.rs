use unigateway_core::{
    ExecutionTarget, GatewayError, OpenAiApiSurfaceCapabilities, ProviderKind, ProviderPool,
    ProxyResponsesRequest, should_retry_responses_without_tools,
};
use unigateway_protocol::{
    ProtocolHttpResponse, render_openai_responses_session,
    render_openai_responses_stream_from_completed,
};

use crate::error::{HostError, HostResult};
use crate::host::HostContext;

use super::dispatch::{should_preserve_stream_error, without_response_tools};
use super::targeting::{build_openai_compatible_target, endpoint_matches_hint};

pub(super) async fn execute_openai_responses_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> HostResult<ProtocolHttpResponse> {
    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;

    let api_surface = resolve_openai_api_surface_for_request(pool, hint, &request.model);

    let response =
        match execute_openai_responses_with_compat(host, target.clone(), request.clone()).await {
            Ok(response) => response,
            Err(error) if should_retry_responses_without_tools(&request, &error, &api_surface) => {
                execute_openai_responses_with_compat(host, target, without_response_tools(request))
                    .await
                    .map_err(HostError::core)?
            }
            Err(error) => return Err(HostError::core(error)),
        };

    Ok(response)
}

async fn execute_openai_responses_with_compat(
    host: &HostContext<'_>,
    target: ExecutionTarget,
    request: ProxyResponsesRequest,
) -> Result<ProtocolHttpResponse, GatewayError> {
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

fn resolve_openai_api_surface_for_request(
    pool: &ProviderPool,
    hint: Option<&str>,
    model: &str,
) -> OpenAiApiSurfaceCapabilities {
    let endpoint = pool
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .filter(|endpoint| endpoint.provider_kind == ProviderKind::OpenAiCompatible)
        .find(|endpoint| {
            hint.map(|hint| endpoint_matches_hint(endpoint, hint))
                .unwrap_or(true)
        });

    OpenAiApiSurfaceCapabilities::resolve_for_model(
        model,
        endpoint.and_then(|endpoint| endpoint.capabilities.openai_api_surface()),
    )
}
