use crate::types::GatewayRequestState;
use anyhow::Result;
use unigateway_core::ProviderPool;
use unigateway_core::UniGatewayEngine;
use unigateway_host::host::{EngineHost, HostEnvProvider, HostFuture, PoolHost, build_env_pool};

impl EngineHost for GatewayRequestState {
    fn core_engine(&self) -> &UniGatewayEngine {
        self.engine()
    }
}

impl PoolHost for GatewayRequestState {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, Result<Option<ProviderPool>>> {
        Box::pin(async move { Ok(self.engine().get_pool(service_id).await) })
    }

    fn env_pool<'a>(
        &'a self,
        provider: HostEnvProvider,
        api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, Result<Option<ProviderPool>>> {
        Box::pin(async move {
            let (base_url, env_api_key, default_model) = match provider {
                HostEnvProvider::OpenAi => (
                    self.provider_base_url(HostEnvProvider::OpenAi),
                    self.provider_api_key(HostEnvProvider::OpenAi),
                    self.provider_model(HostEnvProvider::OpenAi),
                ),
                HostEnvProvider::Anthropic => (
                    self.provider_base_url(HostEnvProvider::Anthropic),
                    self.provider_api_key(HostEnvProvider::Anthropic),
                    self.provider_model(HostEnvProvider::Anthropic),
                ),
            };

            let api_key = api_key_override
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(env_api_key);

            if api_key.trim().is_empty() {
                return Ok(None);
            }

            let pool = build_env_pool(provider, default_model, base_url, api_key);

            self.engine()
                .upsert_pool(pool.clone())
                .await
                .map_err(|error| anyhow::Error::msg(error.to_string()))?;

            Ok(Some(pool))
        })
    }
}
