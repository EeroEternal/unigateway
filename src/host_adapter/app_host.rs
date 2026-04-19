use crate::types::GatewayRequestState;
use unigateway_host::env::{EnvPoolHost, EnvProvider, build_env_pool};
use unigateway_host::error::{PoolLookupError, PoolLookupResult};
use unigateway_host::host::{HostFuture, PoolHost, PoolLookupOutcome};

impl PoolHost for GatewayRequestState {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            Ok(match self.engine().get_pool(service_id).await {
                Some(pool) => PoolLookupOutcome::found(pool),
                None => PoolLookupOutcome::not_found(),
            })
        })
    }
}

impl EnvPoolHost for GatewayRequestState {
    fn env_pool<'a>(
        &'a self,
        provider: EnvProvider,
        api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            let (base_url, env_api_key, default_model) = match provider {
                EnvProvider::OpenAi => (
                    self.provider_base_url(EnvProvider::OpenAi),
                    self.provider_api_key(EnvProvider::OpenAi),
                    self.provider_model(EnvProvider::OpenAi),
                ),
                EnvProvider::Anthropic => (
                    self.provider_base_url(EnvProvider::Anthropic),
                    self.provider_api_key(EnvProvider::Anthropic),
                    self.provider_model(EnvProvider::Anthropic),
                ),
            };

            let api_key = api_key_override
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(env_api_key);

            if api_key.trim().is_empty() {
                return Ok(PoolLookupOutcome::not_found());
            }

            let pool = build_env_pool(provider, default_model, base_url, api_key);

            self.engine()
                .upsert_pool(pool.clone())
                .await
                .map_err(PoolLookupError::other)?;

            Ok(PoolLookupOutcome::found(pool))
        })
    }
}
