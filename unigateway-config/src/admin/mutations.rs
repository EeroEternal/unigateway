//! State-mutating admin operations on `GatewayState`: config values,
//! services, providers, bindings, and model options.

use anyhow::Result;

use crate::routing::normalize_base_url;
use crate::{
    BindingEntry, GatewayState, ProviderEntry, ProviderModelOptions, ServiceEntry,
    default_round_robin,
};

impl GatewayState {
    pub async fn set_config_value(&self, key: &str, value: &str) -> Result<()> {
        let mut guard = self.write_config().await;
        match key {
            "preferences.default_mode" => {
                guard.file.preferences.default_mode = value.to_string();
            }
            _ => anyhow::bail!("unknown config key '{}'", key),
        }
        guard.dirty = true;
        Ok(())
    }

    pub async fn set_default_mode(&self, mode_id: &str) -> Result<()> {
        let mut guard = self.write_config().await;
        if !guard
            .file
            .services
            .iter()
            .any(|service| service.id == mode_id)
        {
            anyhow::bail!("mode '{}' not found", mode_id);
        }
        guard.file.preferences.default_mode = mode_id.to_string();
        guard.dirty = true;
        Ok(())
    }

    pub async fn create_service(&self, id: &str, name: &str) {
        {
            let mut guard = self.write_config().await;
            if let Some(s) = guard.file.services.iter_mut().find(|s| s.id == id) {
                s.name = name.to_string();
            } else {
                guard.file.services.push(ServiceEntry {
                    id: id.to_string(),
                    name: name.to_string(),
                    routing_strategy: default_round_robin(),
                });
            }
            guard.dirty = true;
        }
        self.request_core_sync().await;
    }

    pub async fn set_service_routing_strategy(
        &self,
        service_id: &str,
        routing_strategy: &str,
    ) -> Result<()> {
        {
            let mut guard = self.write_config().await;
            let Some(service) = guard
                .file
                .services
                .iter_mut()
                .find(|service| service.id == service_id)
            else {
                anyhow::bail!("service '{}' not found", service_id);
            };
            service.routing_strategy = routing_strategy.to_string();
            guard.dirty = true;
        }
        self.request_core_sync().await;
        Ok(())
    }

    pub async fn update_provider_by_name(
        &self,
        name: &str,
        base_url: Option<&str>,
        api_key: Option<&str>,
        default_model: Option<&str>,
        model_mapping: Option<&str>,
    ) -> Result<()> {
        let mut guard = self.write_config().await;
        let provider = guard
            .file
            .providers
            .iter_mut()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("provider '{}' not found", name))?;

        if let Some(url) = base_url {
            let endpoint_id = provider.endpoint_id.as_str();
            let mut final_base_url = normalize_base_url(url);
            if !endpoint_id.is_empty()
                && let Some((_, endpoint)) = llm_providers::get_endpoint(endpoint_id)
            {
                let default_url = normalize_base_url(endpoint.base_url);
                if final_base_url == default_url {
                    final_base_url = String::new();
                }
            }
            provider.base_url = final_base_url;
        }
        if let Some(key) = api_key {
            provider.api_key = key.to_string();
        }
        if let Some(model) = default_model {
            provider.default_model = model.to_string();
        }
        if let Some(mapping) = model_mapping {
            provider.model_mapping = mapping.to_string();
        }
        guard.dirty = true;
        drop(guard);
        self.request_core_sync().await;
        Ok(())
    }

    pub async fn delete_provider_by_name(&self, name: &str) -> Result<()> {
        let mut guard = self.write_config().await;
        let before = guard.file.providers.len();
        guard.file.providers.retain(|p| p.name != name);
        if guard.file.providers.len() == before {
            anyhow::bail!("provider '{}' not found", name);
        }
        guard.file.bindings.retain(|b| b.provider_name != name);
        guard.dirty = true;
        drop(guard);
        self.request_core_sync().await;
        Ok(())
    }

    pub async fn create_provider(
        &self,
        name: &str,
        provider_type: &str,
        endpoint_id: &str,
        base_url: Option<&str>,
        api_key: &str,
        model_mapping: Option<&str>,
    ) -> i64 {
        self.create_provider_with_models(
            name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            ProviderModelOptions {
                default_model: None,
                model_mapping,
                metadata: None,
            },
        )
        .await
    }

    pub async fn create_provider_with_models(
        &self,
        name: &str,
        provider_type: &str,
        endpoint_id: &str,
        base_url: Option<&str>,
        api_key: &str,
        model_options: ProviderModelOptions<'_>,
    ) -> i64 {
        let idx = {
            let mut guard = self.write_config().await;

            // If base_url is provided but matches the default base_url for this endpoint_id,
            // we store it as empty to keep config.toml clean and rely on single source of truth.
            let mut final_base_url = base_url.map(normalize_base_url).unwrap_or_default();
            if !endpoint_id.is_empty()
                && let Some((_, endpoint)) = llm_providers::get_endpoint(endpoint_id)
            {
                let default_url = normalize_base_url(endpoint.base_url);
                if final_base_url == default_url {
                    final_base_url = String::new();
                }
            }

            let entry = ProviderEntry {
                name: name.to_string(),
                provider_type: provider_type.to_string(),
                endpoint_id: endpoint_id.to_string(),
                base_url: final_base_url,
                api_key: api_key.to_string(),
                default_model: model_options.default_model.unwrap_or("").to_string(),
                model_mapping: model_options.model_mapping.unwrap_or("").to_string(),
                is_enabled: true,
                metadata: model_options.metadata.unwrap_or_default(),
            };
            let idx = if let Some((i, p)) = guard
                .file
                .providers
                .iter_mut()
                .enumerate()
                .find(|(_, p)| p.name == name)
            {
                *p = entry;
                i as i64
            } else {
                let i = guard.file.providers.len() as i64;
                guard.file.providers.push(entry);
                i
            };
            guard.dirty = true;
            idx
        };
        self.request_core_sync().await;
        idx
    }

    pub async fn bind_provider_to_service(&self, service_id: &str, provider_id: i64) -> Result<()> {
        self.bind_provider_to_service_with_priority(service_id, provider_id, 0)
            .await
    }

    pub async fn bind_provider_to_service_with_priority(
        &self,
        service_id: &str,
        provider_id: i64,
        priority: i64,
    ) -> Result<()> {
        let provider_name = {
            let guard = self.read_config().await;
            let idx = provider_id as usize;
            guard.file.providers.get(idx).map(|p| p.name.clone())
        };
        let Some(provider_name) = provider_name else {
            anyhow::bail!("provider_id {} not found", provider_id);
        };
        {
            let mut guard = self.write_config().await;
            let exists = guard
                .file
                .bindings
                .iter()
                .any(|b| b.service_id == service_id && b.provider_name == provider_name);
            if let Some(binding) = guard.file.bindings.iter_mut().find(|binding| {
                binding.service_id == service_id && binding.provider_name == provider_name
            }) {
                binding.priority = priority;
                guard.dirty = true;
            } else if !exists {
                guard.file.bindings.push(BindingEntry {
                    service_id: service_id.to_string(),
                    provider_name,
                    priority,
                });
                guard.dirty = true;
            }
        }
        self.request_core_sync().await;
        Ok(())
    }

    pub async fn set_provider_model_options(
        &self,
        provider_id: i64,
        options: ProviderModelOptions<'_>,
    ) -> Result<()> {
        let mut guard = self.write_config().await;
        let p = guard
            .file
            .providers
            .get_mut(provider_id as usize)
            .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if let Some(m) = options.default_model {
            p.default_model = m.to_string();
        }
        if let Some(m) = options.model_mapping {
            p.model_mapping = m.to_string();
        }
        guard.dirty = true;
        Ok(())
    }
}
