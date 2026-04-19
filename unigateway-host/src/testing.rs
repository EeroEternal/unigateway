use std::collections::HashMap;

use unigateway_core::{ProviderPool, UniGatewayEngine};

use crate::env::{EnvPoolHost, EnvProvider};
use crate::error::PoolLookupResult;
use crate::host::{HostContext, HostFuture, PoolHost, PoolLookupOutcome};

/// Simple in-memory host fixture for embedder integration tests.
#[derive(Default)]
pub struct MockHost {
    service_pools: HashMap<String, PoolLookupOutcome>,
    env_pools: HashMap<EnvProvider, PoolLookupOutcome>,
}

impl MockHost {
    /// Create an empty mock host where all lookups default to `NotFound`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a service-backed pool.
    pub fn with_service_pool(mut self, service_id: impl Into<String>, pool: ProviderPool) -> Self {
        self.service_pools
            .insert(service_id.into(), PoolLookupOutcome::found(pool));
        self
    }

    /// Register an explicit service lookup outcome.
    pub fn with_service_outcome(
        mut self,
        service_id: impl Into<String>,
        outcome: PoolLookupOutcome,
    ) -> Self {
        self.service_pools.insert(service_id.into(), outcome);
        self
    }

    /// Register an env-backed pool for a provider family.
    pub fn with_env_pool(mut self, provider: EnvProvider, pool: ProviderPool) -> Self {
        self.env_pools
            .insert(provider, PoolLookupOutcome::found(pool));
        self
    }

    /// Register an explicit env lookup outcome.
    pub fn with_env_outcome(mut self, provider: EnvProvider, outcome: PoolLookupOutcome) -> Self {
        self.env_pools.insert(provider, outcome);
        self
    }
}

impl PoolHost for MockHost {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            Ok(self
                .service_pools
                .get(service_id)
                .cloned()
                .unwrap_or_else(PoolLookupOutcome::not_found))
        })
    }
}

impl EnvPoolHost for MockHost {
    fn env_pool<'a>(
        &'a self,
        provider: EnvProvider,
        _api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            Ok(self
                .env_pools
                .get(&provider)
                .cloned()
                .unwrap_or_else(PoolLookupOutcome::not_found))
        })
    }
}

/// Build a `HostContext` for a mock host fixture.
pub fn build_context<'a>(engine: &'a UniGatewayEngine, host: &'a MockHost) -> HostContext<'a> {
    HostContext::from_parts(engine, host)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{LoadBalancingStrategy, ProviderPool, RetryPolicy, UniGatewayEngine};

    use super::{MockHost, build_context};
    use crate::env::EnvProvider;
    use crate::host::PoolLookupOutcome;

    fn test_pool(pool_id: &str) -> ProviderPool {
        ProviderPool {
            pool_id: pool_id.to_string(),
            endpoints: Vec::new(),
            load_balancing: LoadBalancingStrategy::RoundRobin,
            retry_policy: RetryPolicy::default(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn mock_host_resolves_service_and_env_pools() {
        let engine = UniGatewayEngine::builder()
            .with_builtin_http_drivers()
            .build()
            .expect("engine");
        let host = MockHost::new()
            .with_service_pool("svc-main", test_pool("pool:svc-main"))
            .with_env_pool(EnvProvider::OpenAi, test_pool("pool:env-openai"));

        let context = build_context(&engine, &host);

        assert!(matches!(
            context.pool_for_service("svc-main").await.expect("service"),
            PoolLookupOutcome::Found(_)
        ));
        assert!(matches!(
            crate::env::EnvPoolHost::env_pool(&host, EnvProvider::OpenAi, None)
                .await
                .expect("env"),
            PoolLookupOutcome::Found(_)
        ));
    }

    #[tokio::test]
    async fn mock_host_defaults_to_not_found() {
        let host = MockHost::new();

        assert_eq!(
            crate::env::EnvPoolHost::env_pool(&host, EnvProvider::Anthropic, None)
                .await
                .expect("env"),
            PoolLookupOutcome::NotFound
        );
    }
}
