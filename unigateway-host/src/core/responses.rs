use unigateway_core::{ExecutionTarget, GatewayError, ProviderPool, ProxyResponsesRequest};
use unigateway_protocol::{
    ProtocolHttpResponse, render_openai_responses_session,
    render_openai_responses_stream_from_completed,
};

use crate::error::{HostError, HostResult};
use crate::host::HostContext;

use super::dispatch::{
    should_preserve_stream_error, should_retry_responses_without_tools, without_response_tools,
};
use super::targeting::build_openai_compatible_target;

pub(super) async fn execute_openai_responses_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyResponsesRequest,
) -> HostResult<ProtocolHttpResponse> {
    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;

    let response =
        match execute_openai_responses_with_compat(host, target.clone(), request.clone()).await {
            Ok(response) => response,
            Err(_) if should_retry_responses_without_tools(&request) => {
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
