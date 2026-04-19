use anyhow::{Result, anyhow};
use unigateway_core::ProxyEmbeddingsRequest;
use unigateway_protocol::{RuntimeHttpResponse, render_openai_embeddings_response};

use crate::host::{HostContext, HostPoolSource};

use super::targeting::{build_openai_compatible_target, prepare_host_pool};

pub async fn try_openai_embeddings_via_core(
    host: &HostContext<'_>,
    source: HostPoolSource<'_>,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
) -> Result<Option<RuntimeHttpResponse>> {
    let pool = match prepare_host_pool(host, source).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;
    let response = host
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(render_openai_embeddings_response(response)))
}
