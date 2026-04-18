use anyhow::{Result, anyhow};
use axum::{
    Json,
    response::{IntoResponse, Response},
};
use unigateway_core::{CompletedResponse, EmbeddingsResponse, ProxyEmbeddingsRequest};

use crate::host::RuntimeContext;

use super::targeting::{build_env_openai_pool, build_openai_compatible_target, prepare_core_pool};

pub async fn try_openai_embeddings_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => {
            return Err(anyhow!(
                "no provider pool available for service '{service_id}'"
            ));
        }
    };

    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;
    let response = runtime
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

pub async fn try_openai_embeddings_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: ProxyEmbeddingsRequest,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    runtime
        .core_engine()
        .upsert_pool(pool.clone())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let target = build_openai_compatible_target(&pool.endpoints, &pool.pool_id, hint)?;
    let response = runtime
        .core_engine()
        .proxy_embeddings(request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

fn embeddings_response_to_openai_response(
    response: CompletedResponse<EmbeddingsResponse>,
) -> Response {
    let raw = response.response.raw;
    let body = if raw.is_object() {
        raw
    } else {
        serde_json::json!({
            "object": "list",
            "data": [],
            "usage": response.report.usage.as_ref().map(|usage| serde_json::json!({
                "prompt_tokens": usage.input_tokens,
                "total_tokens": usage.total_tokens,
            })),
        })
    };

    Json(body).into_response()
}
