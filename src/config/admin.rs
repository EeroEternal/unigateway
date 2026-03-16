use anyhow::Result;

use super::{
    ApiKeyEntry, BindingEntry, GatewayState, ProviderEntry, ProviderModelOptions, ServiceEntry,
    default_round_robin,
};

impl GatewayState {
    pub async fn set_config_value(&self, key: &str, value: &str) -> Result<()> {
        let mut guard = self.inner.write().await;
        match key {
            "preferences.default_mode" => {
                guard.file.preferences.default_mode = value.to_string();
            }
            _ => anyhow::bail!("unknown config key '{}'", key),
        }
        guard.dirty = true;
        Ok(())
    }

    pub async fn get_config_value(&self, key: &str) -> Result<String> {
        let guard = self.inner.read().await;
        match key {
            "preferences.default_mode" => Ok(guard.file.preferences.default_mode.clone()),
            _ => anyhow::bail!("unknown config key '{}'", key),
        }
    }

    pub async fn list_services(&self) -> Vec<(String, String)> {
        let guard = self.inner.read().await;
        guard
            .file
            .services
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    }

    pub async fn get_default_mode(&self) -> Option<String> {
        let guard = self.inner.read().await;
        let default_mode = guard.file.preferences.default_mode.trim();
        if default_mode.is_empty() {
            None
        } else {
            Some(default_mode.to_string())
        }
    }

    pub async fn set_default_mode(&self, mode_id: &str) -> Result<()> {
        let mut guard = self.inner.write().await;
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
        let mut guard = self.inner.write().await;
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

    pub async fn set_service_routing_strategy(
        &self,
        service_id: &str,
        routing_strategy: &str,
    ) -> Result<()> {
        let mut guard = self.inner.write().await;
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
        Ok(())
    }

    pub async fn list_providers(
        &self,
    ) -> Vec<(i64, String, String, Option<String>, Option<String>)> {
        let guard = self.inner.read().await;
        guard
            .file
            .providers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    i as i64,
                    p.name.clone(),
                    p.provider_type.clone(),
                    if p.endpoint_id.is_empty() {
                        None
                    } else {
                        Some(p.endpoint_id.clone())
                    },
                    if p.base_url.is_empty() {
                        None
                    } else {
                        Some(p.base_url.clone())
                    },
                )
            })
            .collect()
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
        let mut guard = self.inner.write().await;
        let entry = ProviderEntry {
            name: name.to_string(),
            provider_type: provider_type.to_string(),
            endpoint_id: endpoint_id.to_string(),
            base_url: base_url.unwrap_or("").to_string(),
            api_key: api_key.to_string(),
            default_model: model_options.default_model.unwrap_or("").to_string(),
            model_mapping: model_options.model_mapping.unwrap_or("").to_string(),
            is_enabled: true,
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
            let guard = self.inner.read().await;
            let idx = provider_id as usize;
            guard.file.providers.get(idx).map(|p| p.name.clone())
        };
        let Some(provider_name) = provider_name else {
            anyhow::bail!("provider_id {} not found", provider_id);
        };
        let mut guard = self.inner.write().await;
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
        Ok(())
    }

    pub async fn list_api_keys(&self) -> Vec<ApiKeyEntry> {
        let guard = self.inner.read().await;
        guard.file.api_keys.clone()
    }

    pub async fn create_api_key(
        &self,
        key: &str,
        service_id: &str,
        quota_limit: Option<i64>,
        qps_limit: Option<f64>,
        concurrency_limit: Option<i64>,
    ) {
        let mut guard = self.inner.write().await;
        let used = guard
            .file
            .api_keys
            .iter()
            .find(|a| a.key == key)
            .map(|a| a.used_quota)
            .unwrap_or(0);
        let entry = ApiKeyEntry {
            key: key.to_string(),
            service_id: service_id.to_string(),
            quota_limit,
            used_quota: used,
            is_active: true,
            qps_limit,
            concurrency_limit,
        };
        if let Some(a) = guard.file.api_keys.iter_mut().find(|a| a.key == key) {
            *a = entry;
        } else {
            guard.file.api_keys.push(entry);
        }
        guard.dirty = true;
    }

    pub async fn set_provider_model_options(
        &self,
        provider_id: i64,
        options: ProviderModelOptions<'_>,
    ) -> Result<()> {
        let mut guard = self.inner.write().await;
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
