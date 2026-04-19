use unigateway_core::{ProviderPool, ProxyEmbeddingsRequest};
use unigateway_protocol::{ProtocolHttpResponse, render_openai_embeddings_response};

use crate::error::{HostError, HostResult};
use crate::host::HostContext;

use super::targeting::build_openai_compatible_target;

pub(super) async fn execute_openai_embeddings_via_core(
    host: &HostContext<'_>,
    pool: &ProviderPool,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
) -> HostResult<ProtocolHttpResponse> {
    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)
        .map_err(HostError::targeting)?;
    let response = host
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(HostError::core)?;

    Ok(render_openai_embeddings_response(response))
}
