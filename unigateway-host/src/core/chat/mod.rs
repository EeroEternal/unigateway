use anyhow::{Result, anyhow};
use unigateway_core::{ProviderPool, ProxyChatRequest};
use unigateway_protocol::{
    RuntimeHttpResponse, render_anthropic_chat_session, render_openai_chat_session,
};

use crate::host::{HostContext, HostPoolSource};

use super::targeting::{build_execution_target, prepare_host_pool};

pub async fn try_anthropic_chat_via_core(
    host: &HostContext<'_>,
    source: HostPoolSource<'_>,
    hint: Option<&str>,
    request: ProxyChatRequest,
    requested_model: &str,
) -> Result<Option<RuntimeHttpResponse>> {
    let pool = match prepare_host_pool(host, source).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_anthropic_chat_via_core(host, pool, hint, request, requested_model).await
}

pub async fn try_openai_chat_via_core(
    host: &HostContext<'_>,
    source: HostPoolSource<'_>,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> Result<Option<RuntimeHttpResponse>> {
    let pool = match prepare_host_pool(host, source).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_openai_chat_via_core(host, pool, hint, request).await
}

async fn execute_openai_chat_via_core(
    host: &HostContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
) -> Result<Option<RuntimeHttpResponse>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = host
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(render_openai_chat_session(session)))
}

async fn execute_anthropic_chat_via_core(
    host: &HostContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: ProxyChatRequest,
    requested_model: &str,
) -> Result<Option<RuntimeHttpResponse>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    let session = host
        .core_engine()
        .proxy_chat(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(render_anthropic_chat_session(
        session,
        requested_model.to_string(),
    )))
}
