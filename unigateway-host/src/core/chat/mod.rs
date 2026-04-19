use unigateway_core::{ProviderPool, ProxyChatRequest};
use unigateway_protocol::{
    ProtocolHttpResponse, render_anthropic_chat_session, render_openai_chat_session,
};

use crate::error::{HostError, HostResult};
use crate::host::HostContext;

use super::targeting::build_execution_target;

pub(super) async fn execute_openai_chat_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> HostResult<ProtocolHttpResponse> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;
    let session = host
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(HostError::core)?;

    Ok(render_openai_chat_session(session))
}

pub(super) async fn execute_anthropic_chat_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> HostResult<ProtocolHttpResponse> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;
    let session = host
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(HostError::core)?;

    Ok(render_anthropic_chat_session(session))
}
