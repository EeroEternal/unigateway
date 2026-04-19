use anyhow::{Result, anyhow};
use unigateway_core::{
    Endpoint, EndpointRef, ExecutionPlan, ExecutionTarget, ProviderKind, ProviderPool,
};

use crate::host::{HostContext, HostPoolSource};

/// Fetches the pool for `service_id` from the host.
///
/// This is a thin delegation to [`PoolHost::pool_for_service`].
///
/// Config-backed pools returned here are expected to have been registered during startup or
/// hot-reload sync. Env-backed fallback pools travel through [`PoolHost::env_pool`]
/// instead and may be materialized on demand by the embedder.
pub(super) async fn prepare_host_pool(
    host: &HostContext<'_>,
    source: HostPoolSource<'_>,
) -> Result<Option<ProviderPool>> {
    host.resolve_pool(source).await
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
            .provider_name
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .source_endpoint_id
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
        || endpoint
            .provider_family
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(hint))
}
