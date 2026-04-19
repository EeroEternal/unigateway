use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use unigateway_core::{
    Endpoint, LoadBalancingStrategy, ModelPolicy, ProviderKind, ProviderPool, RetryPolicy,
    SecretString, UniGatewayEngine,
};

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEnvProvider {
    OpenAi,
    Anthropic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPoolSource<'a> {
    Service(&'a str),
    Env {
        provider: HostEnvProvider,
        api_key_override: Option<&'a str>,
    },
}

impl HostEnvProvider {
    pub fn pool_id(self) -> &'static str {
        match self {
            Self::OpenAi => "__env_openai__",
            Self::Anthropic => "__env_anthropic__",
        }
    }

    pub fn endpoint_id(self) -> &'static str {
        match self {
            Self::OpenAi => "env-openai",
            Self::Anthropic => "env-anthropic",
        }
    }

    pub fn provider_name(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn provider_kind(self) -> ProviderKind {
        match self {
            Self::OpenAi => ProviderKind::OpenAiCompatible,
            Self::Anthropic => ProviderKind::Anthropic,
        }
    }

    pub fn driver_id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai-compatible",
            Self::Anthropic => "anthropic",
        }
    }
}

#[derive(Clone, Copy)]
pub struct HostContext<'a> {
    engine_host: &'a dyn EngineHost,
    pool_host: &'a dyn PoolHost,
}

pub trait EngineHost: Send + Sync {
    fn core_engine(&self) -> &UniGatewayEngine;
}

/// Provides per-request access to the pool that should serve a given service.
///
/// # Contract for implementors
///
/// The pool returned here **must already be registered** in the engine via
/// [`UniGatewayEngine::upsert_pool`] before this method is called. The host
/// execution helpers re-exported from [`crate::core`] build an
/// [`ExecutionTarget::Pool`][unigateway_core::ExecutionTarget] from the returned pool id and
/// then ask the engine to resolve it — if the pool has not been upserted the engine
/// will return [`GatewayError::PoolNotFound`][unigateway_core::GatewayError::PoolNotFound].
///
/// The recommended lifecycle for embedders is:
///
/// 1. **Startup sync** — call `engine.upsert_pool(pool)` for every pool fetched from
///    your datastore.
/// 2. **Hot updates** — whenever a pool changes, call `engine.upsert_pool(pool)` or
///    `engine.remove_pool(pool_id)`.
/// 3. **Per-request** — implement this method as a fast in-memory look-up that returns
///    `engine.get_pool(service_id)` (or equivalent).  Do **not** query an external
///    datastore on every request.
pub trait PoolHost: Send + Sync {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, Result<Option<ProviderPool>>>;

    /// Materializes an env-backed fallback pool for the current request.
    ///
    /// Unlike [`PoolHost::pool_for_service`], this method is explicitly allowed to
    /// build and upsert a synthetic pool on demand, as long as the returned pool has been
    /// registered in the engine before the future resolves.
    fn env_pool<'a>(
        &'a self,
        provider: HostEnvProvider,
        api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, Result<Option<ProviderPool>>>;
}

impl<'a> HostContext<'a> {
    pub fn from_parts(engine_host: &'a dyn EngineHost, pool_host: &'a dyn PoolHost) -> Self {
        Self {
            engine_host,
            pool_host,
        }
    }

    pub fn core_engine(&self) -> &UniGatewayEngine {
        self.engine_host.core_engine()
    }

    pub async fn pool_for_service(&self, service_id: &str) -> Result<Option<ProviderPool>> {
        self.pool_host.pool_for_service(service_id).await
    }

    pub async fn env_pool(
        &self,
        provider: HostEnvProvider,
        api_key_override: Option<&str>,
    ) -> Result<Option<ProviderPool>> {
        self.pool_host.env_pool(provider, api_key_override).await
    }

    pub async fn resolve_pool(&self, source: HostPoolSource<'_>) -> Result<Option<ProviderPool>> {
        match source {
            HostPoolSource::Service(service_id) => self.pool_for_service(service_id).await,
            HostPoolSource::Env {
                provider,
                api_key_override,
            } => self.env_pool(provider, api_key_override).await,
        }
    }
}

pub fn build_env_pool(
    provider: HostEnvProvider,
    default_model: &str,
    base_url: &str,
    api_key: &str,
) -> ProviderPool {
    let endpoint_id = provider.endpoint_id().to_string();
    let provider_name = provider.provider_name().to_string();

    ProviderPool {
        pool_id: provider.pool_id().to_string(),
        endpoints: vec![Endpoint {
            endpoint_id: endpoint_id.clone(),
            provider_name: Some(endpoint_id.clone()),
            source_endpoint_id: Some(endpoint_id.clone()),
            provider_family: Some(provider_name.clone()),
            provider_kind: provider.provider_kind(),
            driver_id: provider.driver_id().to_string(),
            base_url: normalize_base_url(base_url),
            api_key: SecretString::new(api_key),
            model_policy: ModelPolicy {
                default_model: Some(default_model.to_string()),
                model_mapping: std::collections::HashMap::new(),
            },
            enabled: true,
            metadata: std::collections::HashMap::new(),
        }],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: RetryPolicy::default(),
        metadata: std::collections::HashMap::from([(
            "service_name".to_string(),
            format!("env-{provider_name}"),
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{LoadBalancingStrategy, ProviderPool, RetryPolicy, UniGatewayEngine};

    use super::{
        EngineHost, HostContext, HostEnvProvider, HostFuture, HostPoolSource, PoolHost,
        build_env_pool,
    };

    struct MockEngineHost {
        engine: UniGatewayEngine,
    }

    impl EngineHost for MockEngineHost {
        fn core_engine(&self) -> &UniGatewayEngine {
            &self.engine
        }
    }

    struct MockPoolHost;

    impl PoolHost for MockPoolHost {
        fn pool_for_service<'a>(
            &'a self,
            service_id: &'a str,
        ) -> HostFuture<'a, anyhow::Result<Option<ProviderPool>>> {
            Box::pin(async move {
                Ok(Some(ProviderPool {
                    pool_id: format!("pool:{service_id}"),
                    endpoints: Vec::new(),
                    load_balancing: LoadBalancingStrategy::RoundRobin,
                    retry_policy: RetryPolicy::default(),
                    metadata: HashMap::new(),
                }))
            })
        }

        fn env_pool<'a>(
            &'a self,
            provider: HostEnvProvider,
            api_key_override: Option<&'a str>,
        ) -> HostFuture<'a, anyhow::Result<Option<ProviderPool>>> {
            Box::pin(async move {
                let api_key = api_key_override.unwrap_or_default();
                if api_key.trim().is_empty() {
                    return Ok(None);
                }

                let default_model = match provider {
                    HostEnvProvider::OpenAi => "gpt-test",
                    HostEnvProvider::Anthropic => "claude-test",
                };

                let base_url = match provider {
                    HostEnvProvider::OpenAi => "https://api.openai.test",
                    HostEnvProvider::Anthropic => "https://api.anthropic.test",
                };

                Ok(Some(build_env_pool(
                    provider,
                    default_model,
                    base_url,
                    api_key,
                )))
            })
        }
    }

    #[tokio::test]
    async fn host_context_can_compose_split_host_capabilities() {
        let engine_host = MockEngineHost {
            engine: UniGatewayEngine::builder()
                .with_builtin_http_drivers()
                .build()
                .unwrap(),
        };
        let pool_host = MockPoolHost;

        let context = HostContext::from_parts(&engine_host, &pool_host);

        assert!(std::ptr::eq(context.core_engine(), &engine_host.engine));

        let pool = context
            .resolve_pool(HostPoolSource::Service("svc-main"))
            .await
            .expect("pool")
            .expect("synced pool");
        assert_eq!(pool.pool_id, "pool:svc-main");

        let env_pool = context
            .resolve_pool(HostPoolSource::Env {
                provider: HostEnvProvider::OpenAi,
                api_key_override: Some("sk-openai"),
            })
            .await
            .expect("env pool")
            .expect("env pool exists");
        assert_eq!(env_pool.pool_id, "__env_openai__");
        assert_eq!(
            env_pool.endpoints[0].provider_name.as_deref(),
            Some("env-openai")
        );
        assert_eq!(
            env_pool.endpoints[0].source_endpoint_id.as_deref(),
            Some("env-openai")
        );
        assert_eq!(
            env_pool.endpoints[0].provider_family.as_deref(),
            Some("openai")
        );
    }

    #[test]
    fn build_env_pool_normalizes_host_specific_defaults() {
        let openai_pool = build_env_pool(
            HostEnvProvider::OpenAi,
            "gpt-4o-mini",
            "https://api.openai.com",
            "sk-openai",
        );
        let anthropic_pool = build_env_pool(
            HostEnvProvider::Anthropic,
            "claude-3-5-sonnet",
            "https://api.anthropic.com/",
            "sk-anthropic",
        );

        assert_eq!(openai_pool.pool_id, "__env_openai__");
        assert_eq!(
            openai_pool.endpoints[0].provider_name.as_deref(),
            Some("env-openai")
        );
        assert_eq!(
            openai_pool.endpoints[0].source_endpoint_id.as_deref(),
            Some("env-openai")
        );
        assert_eq!(
            openai_pool.endpoints[0].provider_family.as_deref(),
            Some("openai")
        );
        assert_eq!(openai_pool.endpoints[0].driver_id, "openai-compatible");
        assert_eq!(openai_pool.endpoints[0].base_url, "https://api.openai.com/");

        assert_eq!(anthropic_pool.pool_id, "__env_anthropic__");
        assert_eq!(
            anthropic_pool.endpoints[0].provider_name.as_deref(),
            Some("env-anthropic")
        );
        assert_eq!(
            anthropic_pool.endpoints[0].source_endpoint_id.as_deref(),
            Some("env-anthropic")
        );
        assert_eq!(
            anthropic_pool.endpoints[0].provider_family.as_deref(),
            Some("anthropic")
        );
        assert_eq!(anthropic_pool.endpoints[0].driver_id, "anthropic");
        assert_eq!(
            anthropic_pool.endpoints[0].base_url,
            "https://api.anthropic.com/"
        );
    }
}
