#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use unigateway_core::{
    Endpoint, LoadBalancingStrategy, ModelPolicy, ProviderKind, ProviderPool, RetryPolicy,
    SecretString, UniGatewayEngine,
};

use super::{BindingEntry, GatewayConfigFile, GatewayState, ProviderEntry, ServiceEntry};
use crate::routing::resolve_upstream;

const CONFIG_MANAGED_POOL_MARKER_KEY: &str = "managed_by";
const CONFIG_MANAGED_POOL_MARKER_VALUE: &str = "gateway-config";

pub(crate) async fn build_core_pool_for_service(
    state: &GatewayState,
    service_id: &str,
) -> Result<ProviderPool> {
    let guard = state.inner.read().await;
    build_pool_from_file(&guard.file, service_id)
}

pub(crate) async fn build_all_core_pools(state: &GatewayState) -> Result<Vec<ProviderPool>> {
    let guard = state.inner.read().await;
    let mut pools = Vec::with_capacity(guard.file.services.len());

    for service in &guard.file.services {
        pools.push(build_pool_from_file(&guard.file, &service.id)?);
    }

    Ok(pools)
}

pub(crate) async fn sync_core_pools(state: &GatewayState, engine: &UniGatewayEngine) -> Result<()> {
    let file = {
        let guard = state.inner.read().await;
        guard.file.clone()
    };

    let service_ids: HashSet<String> = file
        .services
        .iter()
        .map(|service| service.id.clone())
        .collect();

    for service in &file.services {
        match build_pool_from_file(&file, &service.id) {
            Ok(pool) => {
                engine
                    .upsert_pool(pool)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;
            }
            Err(error) => {
                engine
                    .remove_pool(&service.id)
                    .await
                    .map_err(|error| anyhow!(error.to_string()))?;

                let message = error.to_string();
                if message.contains("unsupported core routing strategy")
                    || message.contains("has no enabled providers for core sync")
                {
                    tracing::debug!(service_id = service.id.as_str(), error = %error, "skipping core pool sync for service");
                } else {
                    tracing::warn!(service_id = service.id.as_str(), error = %error, "failed to sync core pool for service");
                }
            }
        }
    }

    for pool in engine.list_pools().await {
        let is_config_managed = pool
            .metadata
            .get(CONFIG_MANAGED_POOL_MARKER_KEY)
            .is_some_and(|value| value == CONFIG_MANAGED_POOL_MARKER_VALUE);

        if is_config_managed && !service_ids.contains(&pool.pool_id) {
            engine
                .remove_pool(&pool.pool_id)
                .await
                .map_err(|error| anyhow!(error.to_string()))?;
        }
    }

    Ok(())
}

fn build_pool_from_file(file: &GatewayConfigFile, service_id: &str) -> Result<ProviderPool> {
    let service = file
        .services
        .iter()
        .find(|service| service.id == service_id)
        .ok_or_else(|| anyhow!("service '{}' not found", service_id))?;

    let load_balancing = to_core_strategy(service)?;
    let mut endpoints = Vec::new();

    for binding in bindings_for_service(file, service_id) {
        let provider = file
            .providers
            .iter()
            .find(|provider| provider.name == binding.provider_name)
            .ok_or_else(|| {
                anyhow!(
                    "provider '{}' bound to service '{}' not found",
                    binding.provider_name,
                    service_id
                )
            })?;

        endpoints.push(to_core_endpoint(service, provider, binding)?);
    }

    if endpoints.is_empty() {
        return Err(anyhow!(
            "service '{}' has no enabled providers for core sync",
            service_id
        ));
    }

    Ok(ProviderPool {
        pool_id: service.id.clone(),
        endpoints,
        load_balancing,
        retry_policy: RetryPolicy::default(),
        metadata: HashMap::from([
            ("service_name".to_string(), service.name.clone()),
            (
                CONFIG_MANAGED_POOL_MARKER_KEY.to_string(),
                CONFIG_MANAGED_POOL_MARKER_VALUE.to_string(),
            ),
        ]),
    })
}

fn bindings_for_service<'a>(
    file: &'a GatewayConfigFile,
    service_id: &str,
) -> impl Iterator<Item = &'a BindingEntry> {
    file.bindings
        .iter()
        .filter(move |binding| binding.service_id == service_id)
}

fn to_core_endpoint(
    service: &ServiceEntry,
    provider: &ProviderEntry,
    binding: &BindingEntry,
) -> Result<Endpoint> {
    if !provider.is_enabled {
        return Err(anyhow!(
            "provider '{}' for service '{}' is disabled",
            provider.name,
            service.id
        ));
    }

    if provider.api_key.trim().is_empty() {
        return Err(anyhow!(
            "provider '{}' for service '{}' is missing api_key",
            provider.name,
            service.id
        ));
    }

    let (base_url, family_id) = resolve_upstream(
        if provider.base_url.is_empty() {
            None
        } else {
            Some(provider.base_url.clone())
        },
        if provider.endpoint_id.is_empty() {
            None
        } else {
            Some(provider.endpoint_id.as_str())
        },
    )
    .ok_or_else(|| {
        anyhow!(
            "provider '{}' for service '{}' has unresolved upstream",
            provider.name,
            service.id
        )
    })?;

    let endpoint_id = if provider.name.trim().is_empty() {
        format!("{}:{}", service.id, binding.priority)
    } else {
        provider.name.clone()
    };

    let mut metadata = HashMap::from([
        ("provider_name".to_string(), provider.name.clone()),
        (
            "source_provider_type".to_string(),
            provider.provider_type.clone(),
        ),
        ("binding_priority".to_string(), binding.priority.to_string()),
    ]);

    if !provider.endpoint_id.is_empty() {
        metadata.insert(
            "source_endpoint_id".to_string(),
            provider.endpoint_id.clone(),
        );
    }

    if let Some(family_id) = family_id {
        metadata.insert("provider_family".to_string(), family_id);
    }

    Ok(Endpoint {
        endpoint_id,
        provider_kind: to_provider_kind(provider),
        driver_id: to_driver_id(provider),
        base_url,
        api_key: SecretString::new(provider.api_key.clone()),
        model_policy: parse_model_policy(provider),
        enabled: true,
        metadata,
    })
}

fn to_core_strategy(service: &ServiceEntry) -> Result<LoadBalancingStrategy> {
    match service.routing_strategy.as_str() {
        "fallback" => Ok(LoadBalancingStrategy::Fallback),
        "round_robin" => Ok(LoadBalancingStrategy::RoundRobin),
        "random" => Ok(LoadBalancingStrategy::Random),
        other => Err(anyhow!(
            "service '{}' uses unsupported core routing strategy '{}'",
            service.id,
            other
        )),
    }
}

fn to_provider_kind(provider: &ProviderEntry) -> ProviderKind {
    match provider.provider_type.as_str() {
        "anthropic" => ProviderKind::Anthropic,
        _ => ProviderKind::OpenAiCompatible,
    }
}

fn to_driver_id(provider: &ProviderEntry) -> String {
    match provider.provider_type.as_str() {
        "anthropic" => "anthropic".to_string(),
        _ => "openai-compatible".to_string(),
    }
}

fn parse_model_policy(provider: &ProviderEntry) -> ModelPolicy {
    let default_model = if provider.default_model.trim().is_empty() {
        None
    } else {
        Some(provider.default_model.clone())
    };

    let model_mapping = parse_model_mapping(&provider.model_mapping);

    ModelPolicy {
        default_model,
        model_mapping,
    }
}

fn parse_model_mapping(raw: &str) -> HashMap<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return HashMap::new();
    }

    if trimmed.starts_with('{') {
        return serde_json::from_str::<HashMap<String, String>>(trimmed).unwrap_or_default();
    }

    HashMap::from([("default".to_string(), trimmed.to_string())])
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use unigateway_core::LoadBalancingStrategy;
    use unigateway_core::UniGatewayEngine;

    use super::{build_all_core_pools, build_core_pool_for_service, sync_core_pools};
    use crate::config::GatewayState;

    #[tokio::test]
    async fn builds_core_pool_for_round_robin_service() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(&config_path).await.expect("load state");

        state.create_service("svc", "Service").await;
        let provider_id = state
            .create_provider(
                "moonshot-main",
                "openai",
                "moonshot:global",
                None,
                "sk-test",
                Some("{\"gpt-4o\":\"moonshot-v1\"}"),
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let pool = build_core_pool_for_service(&state, "svc")
            .await
            .expect("build core pool");

        assert_eq!(pool.pool_id, "svc");
        assert_eq!(pool.endpoints.len(), 1);
        assert_eq!(pool.endpoints[0].driver_id, "openai-compatible");
        assert_eq!(
            pool.endpoints[0]
                .model_policy
                .model_mapping
                .get("gpt-4o")
                .map(String::as_str),
            Some("moonshot-v1")
        );
    }

    #[tokio::test]
    async fn build_all_core_pools_supports_fallback_strategy() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(&config_path).await.expect("load state");

        state.create_service("svc", "Service").await;
        state
            .set_service_routing_strategy("svc", "fallback")
            .await
            .expect("set strategy");
        let provider_id = state
            .create_provider(
                "anthropic-main",
                "anthropic",
                "",
                Some("https://api.anthropic.com"),
                "sk-ant",
                None,
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let pools = build_all_core_pools(&state)
            .await
            .expect("fallback strategy should sync");

        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].load_balancing, LoadBalancingStrategy::Fallback);
    }

    #[tokio::test]
    async fn sync_core_pools_keeps_fallback_services_synced() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(&config_path).await.expect("load state");

        state.create_service("svc-ok", "Supported").await;
        state.create_service("svc-legacy", "Legacy").await;
        state
            .set_service_routing_strategy("svc-legacy", "fallback")
            .await
            .expect("set legacy strategy");

        let provider_id = state
            .create_provider(
                "moonshot-main",
                "openai",
                "moonshot:global",
                None,
                "sk-test",
                None,
            )
            .await;
        state
            .bind_provider_to_service("svc-ok", provider_id)
            .await
            .expect("bind provider");
        state
            .bind_provider_to_service("svc-legacy", provider_id)
            .await
            .expect("bind provider to fallback service");

        let engine = UniGatewayEngine::builder()
            .with_builtin_http_drivers()
            .build()
            .unwrap();
        sync_core_pools(state.as_ref(), &engine)
            .await
            .expect("sync core pools");

        assert!(engine.get_pool("svc-ok").await.is_some());
        assert!(engine.get_pool("svc-legacy").await.is_some());
    }
}
