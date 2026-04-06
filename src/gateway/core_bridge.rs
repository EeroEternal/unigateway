use anyhow::{Result, anyhow};
use axum::response::Response;
use llm_connector::types::{ChatRequest, EmbedRequest, ResponsesRequest};
use unigateway_core::{
    Endpoint, EndpointRef, ExecutionPlan, ExecutionTarget, GatewayError, LoadBalancingStrategy,
    ModelPolicy, ProviderKind, ProviderPool, ProxyEmbeddingsRequest, ProxyResponsesRequest,
    RetryPolicy, SecretString,
};

use crate::routing::normalize_base_url;

use super::adapter::{
    chat_session_to_anthropic_response, chat_session_to_openai_response,
    embeddings_response_to_openai_response, responses_session_to_openai_response,
    to_core_chat_request, to_core_responses_request,
};
use super::context::RuntimeContext;

pub(crate) async fn try_anthropic_chat_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Option<Response>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    runtime
        .core_engine()
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let session = runtime
        .core_engine()
        .proxy_chat(to_core_chat_request(request), target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_anthropic_response(
        session,
        request.model.clone(),
    )))
}

pub(crate) async fn try_openai_chat_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Option<Response>> {
    let pool = match runtime.build_pool_for_service(service_id).await {
        Ok(pool) => pool,
        Err(error)
            if error
                .to_string()
                .contains("unsupported core routing strategy") =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    if pool
        .endpoints
        .iter()
        .any(|endpoint| endpoint.provider_kind != ProviderKind::OpenAiCompatible)
    {
        return Ok(None);
    }

    execute_openai_chat_via_core(runtime, pool, hint, request).await
}

pub(crate) async fn try_openai_chat_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: &ChatRequest,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    execute_openai_chat_via_core(runtime, pool, hint, request).await
}

pub(crate) async fn try_openai_responses_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &ResponsesRequest,
    payload: &serde_json::Value,
) -> Result<Option<Response>> {
    if !responses_payload_is_core_compatible(payload) {
        return Ok(None);
    }

    let pool = match prepare_openai_compatible_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    execute_openai_responses_via_core(runtime, pool, hint, request).await
}

pub(crate) async fn try_openai_responses_via_env_core(
    runtime: &RuntimeContext<'_>,
    hint: Option<&str>,
    request: &ResponsesRequest,
    payload: &serde_json::Value,
    base_url: &str,
    api_key: &str,
) -> Result<Option<Response>> {
    if !responses_payload_is_core_compatible(payload) {
        return Ok(None);
    }

    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        return Ok(None);
    }

    let pool = build_env_openai_pool(runtime.config.openai_model, base_url, api_key);

    execute_openai_responses_via_core(runtime, pool, hint, request).await
}

pub(crate) async fn try_openai_embeddings_via_core(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
    hint: Option<&str>,
    request: &EmbedRequest,
    payload: &serde_json::Value,
) -> Result<Option<Response>> {
    if !embeddings_payload_is_core_compatible(payload) {
        return Ok(None);
    }

    let pool = match prepare_openai_compatible_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    runtime
        .core_engine()
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let response = runtime
        .core_engine()
        .proxy_embeddings(
            ProxyEmbeddingsRequest {
                model: request.model.clone(),
                input: request.input.clone(),
                metadata: std::collections::HashMap::new(),
            },
            target,
        )
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(embeddings_response_to_openai_response(response)))
}

async fn execute_openai_chat_via_core(
    runtime: &RuntimeContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: &ChatRequest,
) -> Result<Option<Response>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    runtime
        .core_engine()
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let core_request = to_core_chat_request(request);
    let session = runtime
        .core_engine()
        .proxy_chat(core_request, target)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    Ok(Some(chat_session_to_openai_response(session)))
}

async fn execute_openai_responses_via_core(
    runtime: &RuntimeContext<'_>,
    pool: ProviderPool,
    hint: Option<&str>,
    request: &ResponsesRequest,
) -> Result<Option<Response>> {
    let target = build_execution_target(&pool.endpoints, &pool.pool_id, hint)?;
    runtime
        .core_engine()
        .upsert_pool(pool)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;

    let core_request = to_core_responses_request(request);
    let session = match runtime
        .core_engine()
        .proxy_responses(core_request.clone(), target.clone())
        .await
    {
        Ok(session) => session,
        Err(error) if should_fallback_to_legacy_responses(&error) => return Ok(None),
        Err(error) if should_retry_responses_without_tools(request) => {
            let retry_request = without_response_tools(core_request);
            match runtime
                .core_engine()
                .proxy_responses(retry_request, target)
                .await
            {
                Ok(session) => session,
                Err(error) if should_fallback_to_legacy_responses(&error) => return Ok(None),
                Err(error) => return Err(anyhow!(error.to_string())),
            }
        }
        Err(error) => return Err(anyhow!(error.to_string())),
    };

    Ok(Some(responses_session_to_openai_response(session)))
}

async fn prepare_openai_compatible_pool(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
) -> Result<Option<ProviderPool>> {
    let pool = match prepare_core_pool(runtime, service_id).await? {
        Some(pool) => pool,
        None => return Ok(None),
    };

    if pool
        .endpoints
        .iter()
        .any(|endpoint| endpoint.provider_kind != ProviderKind::OpenAiCompatible)
    {
        return Ok(None);
    }

    Ok(Some(pool))
}

async fn prepare_core_pool(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
) -> Result<Option<ProviderPool>> {
    match runtime.build_pool_for_service(service_id).await {
        Ok(pool) => Ok(Some(pool)),
        Err(error)
            if error
                .to_string()
                .contains("unsupported core routing strategy") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn build_execution_target(
    endpoints: &[Endpoint],
    pool_id: &str,
    hint: Option<&str>,
) -> Result<ExecutionTarget> {
    let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
        return Ok(ExecutionTarget::Pool {
            pool_id: pool_id.to_string(),
        });
    };

    let candidates: Vec<EndpointRef> = endpoints
        .iter()
        .filter(|endpoint| endpoint_matches_hint(endpoint, hint))
        .map(|endpoint| EndpointRef {
            endpoint_id: endpoint.endpoint_id.clone(),
        })
        .collect();

    if candidates.is_empty() {
        return Err(anyhow!("no provider matches target '{hint}'"));
    }

    Ok(ExecutionTarget::Plan(ExecutionPlan {
        pool_id: Some(pool_id.to_string()),
        candidates,
        load_balancing_override: None,
        retry_policy_override: None,
        metadata: std::collections::HashMap::new(),
    }))
}

fn endpoint_matches_hint(endpoint: &Endpoint, hint: &str) -> bool {
    endpoint.endpoint_id.eq_ignore_ascii_case(hint)
        || endpoint
            .metadata
            .get("provider_name")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .metadata
            .get("source_endpoint_id")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .metadata
            .get("provider_family")
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
}

fn build_env_openai_pool(default_model: &str, base_url: &str, api_key: &str) -> ProviderPool {
    ProviderPool {
        pool_id: "__env_openai__".to_string(),
        endpoints: vec![Endpoint {
            endpoint_id: "env-openai".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: normalize_base_url(base_url),
            api_key: SecretString::new(api_key),
            model_policy: ModelPolicy {
                default_model: Some(default_model.to_string()),
                model_mapping: std::collections::HashMap::new(),
            },
            enabled: true,
            metadata: std::collections::HashMap::from([
                ("provider_name".to_string(), "env-openai".to_string()),
                ("source_endpoint_id".to_string(), "env-openai".to_string()),
                ("provider_family".to_string(), "openai".to_string()),
            ]),
        }],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: std::collections::HashMap::from([(
            "service_name".to_string(),
            "env-openai".to_string(),
        )]),
    }
}

fn without_response_tools(request: ProxyResponsesRequest) -> ProxyResponsesRequest {
    ProxyResponsesRequest {
        tools: None,
        tool_choice: None,
        ..request
    }
}

fn should_retry_responses_without_tools(request: &ResponsesRequest) -> bool {
    request.tools.is_some() || request.tool_choice.is_some()
}

fn should_fallback_to_legacy_responses(error: &GatewayError) -> bool {
    matches!(
        error,
        GatewayError::NotImplemented(_)
            | GatewayError::UpstreamHttp { status: 404, .. }
            | GatewayError::UpstreamHttp { status: 405, .. }
    )
}

fn responses_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.is_object()
}

fn embeddings_payload_is_core_compatible(payload: &serde_json::Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|key| matches!(key.as_str(), "model" | "input"))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{
        Endpoint, GatewayError, ModelPolicy, ProviderKind, ProxyResponsesRequest, SecretString,
    };

    use super::{
        build_env_openai_pool, embeddings_payload_is_core_compatible, endpoint_matches_hint,
        responses_payload_is_core_compatible, should_fallback_to_legacy_responses,
        without_response_tools,
    };

    fn endpoint() -> Endpoint {
        Endpoint {
            endpoint_id: "deepseek-main".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: "https://api.example.com".to_string(),
            api_key: SecretString::new("sk-test"),
            model_policy: ModelPolicy::default(),
            enabled: true,
            metadata: HashMap::from([
                ("provider_name".to_string(), "DeepSeek-Main".to_string()),
                (
                    "source_endpoint_id".to_string(),
                    "deepseek:global".to_string(),
                ),
                ("provider_family".to_string(), "deepseek".to_string()),
            ]),
        }
    }

    #[test]
    fn endpoint_hint_matching_supports_existing_product_forms() {
        let endpoint = endpoint();
        assert!(endpoint_matches_hint(&endpoint, "deepseek-main"));
        assert!(endpoint_matches_hint(&endpoint, "DeepSeek-Main"));
        assert!(endpoint_matches_hint(&endpoint, "deepseek:global"));
        assert!(endpoint_matches_hint(&endpoint, "deepseek"));
        assert!(!endpoint_matches_hint(&endpoint, "zhipu"));
    }

    #[test]
    fn env_openai_pool_matches_basic_openai_hints() {
        let pool = build_env_openai_pool("gpt-4o-mini", "https://api.openai.com", "sk-test");
        let endpoint = pool.endpoints.first().expect("endpoint");

        assert!(endpoint_matches_hint(endpoint, "env-openai"));
        assert!(endpoint_matches_hint(endpoint, "openai"));
        assert!(!endpoint_matches_hint(endpoint, "deepseek"));
    }

    #[test]
    fn responses_core_bridge_accepts_supported_safe_subset() {
        assert!(responses_payload_is_core_compatible(&serde_json::json!({
            "model": "gpt-4.1-mini",
            "input": "hello",
            "stream": true,
            "instructions": "be terse",
            "temperature": 0.2,
            "top_p": 0.9,
            "max_output_tokens": 128,
            "tools": [],
            "tool_choice": "auto",
            "previous_response_id": "resp_prev",
            "metadata": {"trace_id": "abc"},
            "reasoning": {"effort": "high"},
            "target_provider": "deepseek",
        })));
        assert!(!responses_payload_is_core_compatible(&serde_json::json!(
            "hello"
        )));
    }

    #[test]
    fn responses_tool_stripping_clears_tool_fields_only() {
        let request = without_response_tools(ProxyResponsesRequest {
            model: "gpt-4.1-mini".to_string(),
            input: Some(serde_json::json!("hello")),
            instructions: Some("be terse".to_string()),
            temperature: Some(0.1),
            top_p: Some(0.8),
            max_output_tokens: Some(128),
            stream: true,
            tools: Some(serde_json::json!([])),
            tool_choice: Some(serde_json::json!("auto")),
            previous_response_id: Some("resp_prev".to_string()),
            request_metadata: Some(serde_json::json!({"trace_id": "abc"})),
            extra: std::collections::HashMap::new(),
            metadata: HashMap::new(),
        });

        assert!(request.tools.is_none());
        assert!(request.tool_choice.is_none());
        assert_eq!(request.instructions.as_deref(), Some("be terse"));
        assert_eq!(request.previous_response_id.as_deref(), Some("resp_prev"));
    }

    #[test]
    fn responses_legacy_fallback_detects_missing_endpoint() {
        assert!(should_fallback_to_legacy_responses(
            &GatewayError::UpstreamHttp {
                status: 404,
                body: Some("not found".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
        assert!(!should_fallback_to_legacy_responses(
            &GatewayError::UpstreamHttp {
                status: 500,
                body: Some("boom".to_string()),
                endpoint_id: "ep-1".to_string(),
            }
        ));
    }

    #[test]
    fn embeddings_core_bridge_only_accepts_minimal_subset() {
        assert!(embeddings_payload_is_core_compatible(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["hello"],
        })));
        assert!(!embeddings_payload_is_core_compatible(&serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["hello"],
            "encoding_format": "float",
        })));
    }
}
