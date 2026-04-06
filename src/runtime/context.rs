use std::future::Future;
use std::pin::Pin;

use crate::routing::ResolvedProvider;
use anyhow::Result;
use unigateway_core::ProviderPool;
use unigateway_core::UniGatewayEngine;

pub(crate) type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy)]
pub(crate) struct RuntimeConfig<'a> {
    pub openai_base_url: &'a str,
    pub openai_api_key: &'a str,
    pub openai_model: &'a str,
    pub anthropic_base_url: &'a str,
    pub anthropic_api_key: &'a str,
    pub anthropic_model: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct RuntimeContext<'a> {
    pub config: RuntimeConfig<'a>,
    engine_host: &'a dyn RuntimeEngineHost,
    pool_host: &'a dyn RuntimePoolHost,
    routing_host: &'a dyn RuntimeRoutingHost,
}

pub(crate) trait RuntimeConfigHost: Send + Sync {
    fn runtime_config(&self) -> RuntimeConfig<'_>;
}

pub(crate) trait RuntimeEngineHost: Send + Sync {
    fn core_engine(&self) -> &UniGatewayEngine;
}

pub(crate) trait RuntimePoolHost: Send + Sync {
    fn build_pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> RuntimeFuture<'a, Result<ProviderPool>>;
}

pub(crate) trait RuntimeRoutingHost: Send + Sync {
    fn resolve_providers<'a>(
        &'a self,
        service_id: &'a str,
        protocol: &'a str,
        hint: Option<&'a str>,
    ) -> RuntimeFuture<'a, Result<Vec<ResolvedProvider>>>;
}

pub(crate) trait RuntimeHost:
    RuntimeConfigHost + RuntimeEngineHost + RuntimePoolHost + RuntimeRoutingHost
{
}

impl<T> RuntimeHost for T where
    T: RuntimeConfigHost + RuntimeEngineHost + RuntimePoolHost + RuntimeRoutingHost + ?Sized
{
}

impl<'a> RuntimeContext<'a> {
    pub(crate) fn new(host: &'a dyn RuntimeHost) -> Self {
        Self::from_parts(host, host, host, host)
    }

    pub(crate) fn from_parts(
        config_host: &'a dyn RuntimeConfigHost,
        engine_host: &'a dyn RuntimeEngineHost,
        pool_host: &'a dyn RuntimePoolHost,
        routing_host: &'a dyn RuntimeRoutingHost,
    ) -> Self {
        Self {
            config: config_host.runtime_config(),
            engine_host,
            pool_host,
            routing_host,
        }
    }

    pub(crate) fn core_engine(&self) -> &UniGatewayEngine {
        self.engine_host.core_engine()
    }

    pub(crate) async fn build_pool_for_service(&self, service_id: &str) -> Result<ProviderPool> {
        self.pool_host.build_pool_for_service(service_id).await
    }

    pub(crate) async fn resolve_providers(
        &self,
        service_id: &str,
        protocol: &str,
        hint: Option<&str>,
    ) -> Result<Vec<ResolvedProvider>> {
        self.routing_host
            .resolve_providers(service_id, protocol, hint)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{LoadBalancingStrategy, ProviderPool, RetryPolicy, UniGatewayEngine};

    use super::{
        ResolvedProvider, RuntimeConfig, RuntimeConfigHost, RuntimeContext, RuntimeEngineHost,
        RuntimeFuture, RuntimePoolHost, RuntimeRoutingHost,
    };

    struct MockConfigHost;

    impl RuntimeConfigHost for MockConfigHost {
        fn runtime_config(&self) -> RuntimeConfig<'_> {
            RuntimeConfig {
                openai_base_url: "https://api.openai.test",
                openai_api_key: "sk-openai",
                openai_model: "gpt-test",
                anthropic_base_url: "https://api.anthropic.test",
                anthropic_api_key: "sk-anthropic",
                anthropic_model: "claude-test",
            }
        }
    }

    struct MockEngineHost {
        engine: UniGatewayEngine,
    }

    impl RuntimeEngineHost for MockEngineHost {
        fn core_engine(&self) -> &UniGatewayEngine {
            &self.engine
        }
    }

    struct MockPoolHost;

    impl RuntimePoolHost for MockPoolHost {
        fn build_pool_for_service<'a>(
            &'a self,
            service_id: &'a str,
        ) -> RuntimeFuture<'a, anyhow::Result<ProviderPool>> {
            Box::pin(async move {
                Ok(ProviderPool {
                    pool_id: format!("pool:{service_id}"),
                    endpoints: Vec::new(),
                    load_balancing: LoadBalancingStrategy::RoundRobin,
                    retry_policy: RetryPolicy::default(),
                    metadata: HashMap::new(),
                })
            })
        }
    }

    struct MockRoutingHost;

    impl RuntimeRoutingHost for MockRoutingHost {
        fn resolve_providers<'a>(
            &'a self,
            service_id: &'a str,
            protocol: &'a str,
            hint: Option<&'a str>,
        ) -> RuntimeFuture<'a, anyhow::Result<Vec<ResolvedProvider>>> {
            Box::pin(async move {
                Ok(vec![ResolvedProvider {
                    name: hint.unwrap_or("default").to_string(),
                    provider_type: protocol.to_string(),
                    endpoint_id: Some(format!("ep:{service_id}")),
                    base_url: "https://provider.test/".to_string(),
                    api_key: "sk-provider".to_string(),
                    family_id: Some("provider-family".to_string()),
                    default_model: Some("upstream-model".to_string()),
                    model_mapping: None,
                }])
            })
        }
    }

    #[tokio::test]
    async fn runtime_context_can_compose_split_host_capabilities() {
        let config_host = MockConfigHost;
        let engine_host = MockEngineHost {
            engine: UniGatewayEngine::builder().build(),
        };
        let pool_host = MockPoolHost;
        let routing_host = MockRoutingHost;

        let context =
            RuntimeContext::from_parts(&config_host, &engine_host, &pool_host, &routing_host);

        assert_eq!(context.config.openai_model, "gpt-test");
        assert!(std::ptr::eq(context.core_engine(), &engine_host.engine));

        let pool = context
            .build_pool_for_service("svc-main")
            .await
            .expect("pool");
        assert_eq!(pool.pool_id, "pool:svc-main");

        let providers = context
            .resolve_providers("svc-main", "openai", Some("deepseek"))
            .await
            .expect("providers");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "deepseek");
        assert_eq!(providers[0].provider_type, "openai");
        assert_eq!(providers[0].endpoint_id.as_deref(), Some("ep:svc-main"));
    }
}
