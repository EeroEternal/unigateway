use crate::routing::resolve_providers;
use crate::types::AppState;
use anyhow::Result;
use unigateway_core::ProviderPool;
use unigateway_core::UniGatewayEngine;
use unigateway_runtime::host::{
    ResolvedProvider, RuntimeConfig, RuntimeConfigHost, RuntimeEngineHost, RuntimeFuture,
    RuntimePoolHost, RuntimeRoutingHost,
};

impl RuntimeConfigHost for AppState {
    fn runtime_config(&self) -> RuntimeConfig<'_> {
        RuntimeConfig {
            openai_base_url: &self.config.openai_base_url,
            openai_api_key: &self.config.openai_api_key,
            openai_model: &self.config.openai_model,
            anthropic_base_url: &self.config.anthropic_base_url,
            anthropic_api_key: &self.config.anthropic_api_key,
            anthropic_model: &self.config.anthropic_model,
        }
    }
}

impl RuntimeEngineHost for AppState {
    fn core_engine(&self) -> &UniGatewayEngine {
        self.core_engine.as_ref()
    }
}

impl RuntimePoolHost for AppState {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> RuntimeFuture<'a, Result<Option<ProviderPool>>> {
        Box::pin(async move { Ok(self.core_engine.get_pool(service_id).await) })
    }
}

impl RuntimeRoutingHost for AppState {
    fn resolve_providers<'a>(
        &'a self,
        service_id: &'a str,
        protocol: &'a str,
        hint: Option<&'a str>,
    ) -> RuntimeFuture<'a, Result<Vec<ResolvedProvider>>> {
        Box::pin(async move {
            resolve_providers(self.gateway.as_ref(), service_id, protocol, hint)
                .await
                .map_err(anyhow::Error::msg)
        })
    }
}
