use anyhow::{Result, anyhow};
use unigateway_core::{
    Endpoint, EndpointRef, ExecutionPlan, ExecutionTarget, LoadBalancingStrategy, ModelPolicy,
    ProviderKind, ProviderPool, RetryPolicy, SecretString,
};

use crate::host::RuntimeContext;

/// Fetches the pool for `service_id` from the runtime host.
///
/// This is a thin delegation to [`RuntimePoolHost::pool_for_service`]. The returned pool
/// **must** already be present inside the engine's pool table (i.e. previously registered
/// via [`UniGatewayEngine::upsert_pool`]). The caller is responsible for upsert on
/// startup and on every subsequent pool change.
pub(super) async fn prepare_core_pool(
    runtime: &RuntimeContext<'_>,
    service_id: &str,
) -> Result<Option<ProviderPool>> {
    runtime.pool_for_service(service_id).await
}

pub(super) fn build_execution_target(
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

pub(super) fn build_openai_compatible_target(
    endpoints: &[Endpoint],
    pool_id: &str,
    hint: Option<&str>,
) -> Result<ExecutionTarget> {
    let compatible_endpoints: Vec<&Endpoint> = endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .filter(|endpoint| endpoint.provider_kind == ProviderKind::OpenAiCompatible)
        .collect();

    if compatible_endpoints.is_empty() {
        return Err(anyhow!("no openai-compatible provider available"));
    }

    let Some(hint) = hint.map(str::trim).filter(|hint| !hint.is_empty()) else {
        let enabled_count = endpoints.iter().filter(|endpoint| endpoint.enabled).count();
        if compatible_endpoints.len() == enabled_count {
            return Ok(ExecutionTarget::Pool {
                pool_id: pool_id.to_string(),
            });
        }

        return Ok(ExecutionTarget::Plan(ExecutionPlan {
            pool_id: Some(pool_id.to_string()),
            candidates: compatible_endpoints
                .into_iter()
                .map(|endpoint| EndpointRef {
                    endpoint_id: endpoint.endpoint_id.clone(),
                })
                .collect(),
            load_balancing_override: None,
            retry_policy_override: None,
            metadata: std::collections::HashMap::new(),
        }));
    };

    let candidates: Vec<EndpointRef> = compatible_endpoints
        .into_iter()
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

pub(super) fn endpoint_matches_hint(endpoint: &Endpoint, hint: &str) -> bool {
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

pub(super) fn build_env_openai_pool(
    default_model: &str,
    base_url: &str,
    api_key: &str,
) -> ProviderPool {
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

pub(super) fn build_env_anthropic_pool(
    default_model: &str,
    base_url: &str,
    api_key: &str,
) -> ProviderPool {
    ProviderPool {
        pool_id: "__env_anthropic__".to_string(),
        endpoints: vec![Endpoint {
            endpoint_id: "env-anthropic".to_string(),
            provider_kind: ProviderKind::Anthropic,
            driver_id: "anthropic".to_string(),
            base_url: normalize_base_url(base_url),
            api_key: SecretString::new(api_key),
            model_policy: ModelPolicy {
                default_model: Some(default_model.to_string()),
                model_mapping: std::collections::HashMap::new(),
            },
            enabled: true,
            metadata: std::collections::HashMap::from([
                ("provider_name".to_string(), "env-anthropic".to_string()),
                (
                    "source_endpoint_id".to_string(),
                    "env-anthropic".to_string(),
                ),
                ("provider_family".to_string(), "anthropic".to_string()),
            ]),
        }],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: std::collections::HashMap::from([(
            "service_name".to_string(),
            "env-anthropic".to_string(),
        )]),
    }
}

fn normalize_base_url(url: &str) -> String {
    let mut normalized = url.trim().to_string();
    if normalized.is_empty() {
        return normalized;
    }
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}
