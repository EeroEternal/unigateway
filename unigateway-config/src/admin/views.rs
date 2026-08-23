//! Read-only admin views over `GatewayState`: service/provider listings
//! and snapshots consumed by embedder management UIs.

use std::collections::{BTreeMap, HashSet};

use crate::{
    GatewayConfigFile, GatewayState, ModeView, ProviderEntry, ProviderView, ServiceModel,
    build_mode_views,
};

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn models_from_provider_entry(provider: &ProviderEntry) -> Vec<String> {
    let trimmed = provider.model_mapping.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return Vec::new();
    }
    if let Ok(map) = serde_json::from_str::<BTreeMap<String, String>>(trimmed) {
        map.keys()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect()
    } else {
        Vec::new()
    }
}

impl GatewayState {
    pub async fn list_services(&self) -> Vec<(String, String)> {
        let guard = self.read_config().await;
        guard
            .file
            .services
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    }

    pub async fn list_services_with_routing(&self) -> Vec<(String, String, String)> {
        let guard = self.read_config().await;
        guard
            .file
            .services
            .iter()
            .map(|service| {
                (
                    service.id.clone(),
                    service.name.clone(),
                    service.routing_strategy.clone(),
                )
            })
            .collect()
    }

    pub async fn config_snapshot(&self) -> GatewayConfigFile {
        self.read_config().await.file.clone()
    }

    pub async fn get_default_mode(&self) -> Option<String> {
        let guard = self.read_config().await;
        let default_mode = guard.file.preferences.default_mode.trim();
        if default_mode.is_empty() {
            None
        } else {
            Some(default_mode.to_string())
        }
    }

    pub async fn list_mode_views(&self) -> Vec<ModeView> {
        let guard = self.read_config().await;
        let default_mode = guard.file.preferences.default_mode.clone();
        build_mode_views(&guard.file, &default_mode)
    }

    pub async fn list_provider_views(&self) -> Vec<ProviderView> {
        let guard = self.read_config().await;
        guard
            .file
            .providers
            .iter()
            .enumerate()
            .map(|(i, p)| ProviderView {
                id: i as i64,
                name: p.name.clone(),
                provider_type: p.provider_type.clone(),
                endpoint_id: non_empty_string(&p.endpoint_id),
                base_url: non_empty_string(&p.base_url),
                default_model: non_empty_string(&p.default_model),
                models: models_from_provider_entry(p),
                is_enabled: p.is_enabled,
            })
            .collect()
    }

    /// Returns the structured model catalog for a service.
    ///
    /// Provider order follows binding priority (ascending). Aliases within a
    /// provider come from trimmed `model_mapping` keys (lexicographically sorted)
    /// followed by `default_model` if it is non-empty and not already present.
    pub async fn list_service_models(&self, service_id: &str) -> Vec<ServiceModel> {
        let providers = self.select_all_providers_for_service(service_id, "").await;
        let mut result = Vec::new();
        for provider in providers {
            let mut mapping = BTreeMap::new();

            if let Some(raw) = provider.model_mapping.as_deref() {
                let trimmed = raw.trim();
                if trimmed.starts_with('{')
                    && let Ok(parsed) = serde_json::from_str::<BTreeMap<String, String>>(trimmed)
                {
                    mapping = parsed
                        .into_iter()
                        .map(|(k, v)| (k.trim().to_string(), v))
                        .collect();
                }
            }

            let mut seen_aliases = HashSet::new();
            let mut aliases: Vec<String> = Vec::new();

            for key in mapping.keys() {
                let alias = key.trim();
                if !alias.is_empty() && seen_aliases.insert(alias.to_string()) {
                    aliases.push(alias.to_string());
                }
            }
            aliases.sort();

            if let Some(default) = provider.default_model.as_deref() {
                let default = default.trim();
                if !default.is_empty() && seen_aliases.insert(default.to_string()) {
                    aliases.push(default.to_string());
                }
            }

            for alias in aliases {
                let canonical = mapping.get(&alias).cloned();
                result.push(ServiceModel {
                    id: format!("{}/{}", provider.name, alias),
                    alias,
                    canonical,
                    owned_by: provider.name.clone(),
                });
            }
        }
        result
    }

    /// Returns a flat, expanded, deduplicated list of `(id, owned_by)` pairs
    /// suitable for emitting an OpenAI-compatible `/v1/models` response.
    ///
    /// Deduplication happens on the final expanded id set (composite + bare alias).
    /// The first occurrence is retained.
    pub async fn list_service_model_ids(&self, service_id: &str) -> Vec<(String, String)> {
        let models = self.list_service_models(service_id).await;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for model in models {
            for id in model.routing_ids() {
                let id = id.to_string();
                if seen.insert(id.clone()) {
                    result.push((id, model.owned_by.clone()));
                }
            }
        }
        result
    }

    /// Validates that an API key exists and is active without consuming quota
    /// or acquiring runtime limits.
    pub async fn list_providers(
        &self,
    ) -> Vec<(i64, String, String, Option<String>, Option<String>)> {
        let guard = self.read_config().await;
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
}
