use unigateway_core::{
    Endpoint, LoadBalancingStrategy, ModelPolicy, ProviderKind, ProviderPool, RetryPolicy,
    SecretString,
};

use crate::error::PoolLookupResult;
use crate::host::{HostFuture, PoolLookupOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvProvider {
    OpenAi,
    Anthropic,
}

pub trait EnvPoolHost: Send + Sync {
    fn env_pool<'a>(
        &'a self,
        _provider: EnvProvider,
        _api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async { Ok(PoolLookupOutcome::not_found()) })
    }
}

impl EnvProvider {
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

pub fn build_env_pool(
    provider: EnvProvider,
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

    use unigateway_core::RetryPolicy;

    use crate::host::PoolLookupOutcome;

    use super::{EnvPoolHost, EnvProvider, PoolLookupResult, build_env_pool};

    struct MockEnvPoolHost;

    impl EnvPoolHost for MockEnvPoolHost {
        fn env_pool<'a>(
            &'a self,
            provider: EnvProvider,
            api_key_override: Option<&'a str>,
        ) -> crate::host::HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
            Box::pin(async move {
                let api_key = api_key_override.unwrap_or_default();
                if api_key.trim().is_empty() {
                    return Ok(PoolLookupOutcome::not_found());
                }

                let default_model = match provider {
                    EnvProvider::OpenAi => "gpt-test",
                    EnvProvider::Anthropic => "claude-test",
                };

                let base_url = match provider {
                    EnvProvider::OpenAi => "https://api.openai.test",
                    EnvProvider::Anthropic => "https://api.anthropic.test",
                };

                Ok(PoolLookupOutcome::found(build_env_pool(
                    provider,
                    default_model,
                    base_url,
                    api_key,
                )))
            })
        }
    }

    struct MockNoEnvHost;

    impl EnvPoolHost for MockNoEnvHost {}

    #[tokio::test]
    async fn env_pool_defaults_to_none_for_embedders_without_env_support() {
        let host = MockNoEnvHost;
        let env_pool = host
            .env_pool(EnvProvider::OpenAi, Some("sk-openai"))
            .await
            .expect("env pool resolution succeeds");

        assert_eq!(env_pool, PoolLookupOutcome::NotFound);
    }

    #[tokio::test]
    async fn env_pool_host_can_materialize_env_pools() {
        let host = MockEnvPoolHost;
        let env_pool = host
            .env_pool(EnvProvider::OpenAi, Some("sk-openai"))
            .await
            .expect("env pool resolution succeeds");

        let PoolLookupOutcome::Found(env_pool) = env_pool else {
            panic!("env pool exists");
        };

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
            EnvProvider::OpenAi,
            "gpt-4o-mini",
            "https://api.openai.com",
            "sk-openai",
        );
        let anthropic_pool = build_env_pool(
            EnvProvider::Anthropic,
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

        let expected_metadata =
            HashMap::from([("service_name".to_string(), "env-openai".to_string())]);
        assert_eq!(openai_pool.metadata, expected_metadata);
        assert_eq!(anthropic_pool.retry_policy, RetryPolicy::default());
    }
}
