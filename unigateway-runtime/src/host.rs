use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use serde_json::Value;
use unigateway_core::{ProviderPool, UniGatewayEngine};

pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProvider {
    pub name: String,
    pub provider_type: String,
    pub endpoint_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    pub family_id: Option<String>,
    pub default_model: Option<String>,
    pub model_mapping: Option<String>,
}

impl ResolvedProvider {
    pub fn map_model(&self, original_model: &str) -> String {
        if self.provider_type == "volcengine"
            && let Some(ref endpoint_id) = self.endpoint_id
            && !endpoint_id.is_empty()
            && !endpoint_id.contains(':')
        {
            return endpoint_id.clone();
        }

        map_model_name(self.model_mapping.as_deref(), original_model)
            .or_else(|| self.default_model.clone())
            .unwrap_or_else(|| original_model.to_string())
    }
}

#[derive(Clone, Copy)]
pub struct RuntimeConfig<'a> {
    pub openai_base_url: &'a str,
    pub openai_api_key: &'a str,
    pub openai_model: &'a str,
    pub anthropic_base_url: &'a str,
    pub anthropic_api_key: &'a str,
    pub anthropic_model: &'a str,
}

#[derive(Clone, Copy)]
pub struct RuntimeContext<'a> {
    pub config: RuntimeConfig<'a>,
    engine_host: &'a dyn RuntimeEngineHost,
    pool_host: &'a dyn RuntimePoolHost,
    routing_host: &'a dyn RuntimeRoutingHost,
}

pub trait RuntimeConfigHost: Send + Sync {
    fn runtime_config(&self) -> RuntimeConfig<'_>;
}

pub trait RuntimeEngineHost: Send + Sync {
    fn core_engine(&self) -> &UniGatewayEngine;
}

/// Provides per-request access to the pool that should serve a given service.
///
/// # Contract for implementors
///
/// The pool returned here **must already be registered** in the engine via
/// [`UniGatewayEngine::upsert_pool`] before this method is called.  The runtime
/// core functions (`try_openai_chat_via_core`, etc.) build an
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
pub trait RuntimePoolHost: Send + Sync {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> RuntimeFuture<'a, Result<Option<ProviderPool>>>;
}

pub trait RuntimeRoutingHost: Send + Sync {
    fn resolve_providers<'a>(
        &'a self,
        service_id: &'a str,
        protocol: &'a str,
        hint: Option<&'a str>,
    ) -> RuntimeFuture<'a, Result<Vec<ResolvedProvider>>>;
}

impl<'a> RuntimeContext<'a> {
    pub fn from_parts(
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

    pub fn core_engine(&self) -> &UniGatewayEngine {
        self.engine_host.core_engine()
    }

    pub async fn pool_for_service(&self, service_id: &str) -> Result<Option<ProviderPool>> {
        self.pool_host.pool_for_service(service_id).await
    }

    pub async fn resolve_providers(
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

fn map_model_name(model_mapping: Option<&str>, requested_model: &str) -> Option<String> {
    let raw_mapping = model_mapping?;

    if let Ok(value) = serde_json::from_str::<Value>(raw_mapping) {
        if let Some(mapped) = value.get(requested_model).and_then(Value::as_str) {
            return Some(mapped.to_string());
        }
        if let Some(default) = value.get("default").and_then(Value::as_str) {
            return Some(default.to_string());
        }
    }

    if !raw_mapping.trim().is_empty() && !raw_mapping.trim().starts_with('{') {
        return Some(raw_mapping.trim().to_string());
    }

    None
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
        fn pool_for_service<'a>(
            &'a self,
            service_id: &'a str,
        ) -> RuntimeFuture<'a, anyhow::Result<Option<ProviderPool>>> {
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
            engine: UniGatewayEngine::builder()
                .with_builtin_http_drivers()
                .build()
                .unwrap(),
        };
        let pool_host = MockPoolHost;
        let routing_host = MockRoutingHost;

        let context =
            RuntimeContext::from_parts(&config_host, &engine_host, &pool_host, &routing_host);

        assert_eq!(context.config.openai_model, "gpt-test");
        assert!(std::ptr::eq(context.core_engine(), &engine_host.engine));

        let pool = context
            .pool_for_service("svc-main")
            .await
            .expect("pool")
            .expect("synced pool");
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

    #[test]
    fn resolved_provider_map_model_prefers_specific_then_default() {
        let provider = ResolvedProvider {
            name: "provider".to_string(),
            provider_type: "openai".to_string(),
            endpoint_id: None,
            base_url: "https://provider.test/".to_string(),
            api_key: "sk-provider".to_string(),
            family_id: None,
            default_model: Some("fallback-model".to_string()),
            model_mapping: Some(r#"{"gpt-4o":"mapped-4o","default":"mapped-default"}"#.to_string()),
        };

        assert_eq!(provider.map_model("gpt-4o"), "mapped-4o");
        assert_eq!(provider.map_model("gpt-5"), "mapped-default");
    }
}
