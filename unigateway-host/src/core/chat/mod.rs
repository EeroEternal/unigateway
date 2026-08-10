use unigateway_core::{
    ChatResponseChunk, ChatResponseFinal, ProviderPool, ProxyChatRequest, ProxySession,
};
use unigateway_protocol::{
    ProtocolHttpResponse, render_anthropic_chat_session, render_openai_chat_session,
};

use crate::error::{HostError, HostResult};
use crate::host::HostContext;
use crate::middleware::HostMiddleware;

use super::targeting::build_execution_target;

pub(super) async fn execute_openai_chat_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
    middleware: Option<&HostMiddleware>,
) -> HostResult<ProtocolHttpResponse> {
    execute_chat_via_core(
        host,
        pool,
        hint,
        request,
        middleware,
        render_openai_chat_session,
    )
    .await
}

pub(super) async fn execute_anthropic_chat_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
    middleware: Option<&HostMiddleware>,
) -> HostResult<ProtocolHttpResponse> {
    execute_chat_via_core(
        host,
        pool,
        hint,
        request,
        middleware,
        render_anthropic_chat_session,
    )
    .await
}

async fn execute_chat_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    mut request: ProxyChatRequest,
    middleware: Option<&HostMiddleware>,
    render: fn(ProxySession<ChatResponseChunk, ChatResponseFinal>) -> ProtocolHttpResponse,
) -> HostResult<ProtocolHttpResponse> {
    if let Some(middleware) = middleware {
        middleware.run_request(host, &mut request).await?;
    }

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;

    let response_request = middleware.map(|_| request.clone());
    let mut session = host
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(HostError::core)?;

    if let (Some(middleware), Some(response_request)) = (middleware, response_request) {
        middleware
            .run_response(host, &response_request, &mut session)
            .await?;
    }

    Ok(render(session))
}
