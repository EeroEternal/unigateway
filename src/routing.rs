use axum::http::HeaderMap;
use llm_providers::get_endpoint;
use serde_json::Value;

use crate::config::{GatewayState, ServiceProvider};
use crate::storage::map_model_name;

/// A provider with its upstream URL already resolved and validated.
#[derive(Debug, Clone)]
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
    /// Apply model_mapping to get the upstream model name.
    /// Falls back to default_model if no explicit mapping matches.
    pub fn map_model(&self, original_model: &str) -> String {
        // Volcengine specific logic: endpoint_id (ep-xxx) takes precedence over model
        if self.provider_type == "volcengine" {
            if let Some(ref eid) = self.endpoint_id {
                if !eid.is_empty() && !eid.contains(':') {
                    return eid.clone();
                }
            }
        }

        map_model_name(self.model_mapping.as_deref(), original_model)
            .or_else(|| self.default_model.clone())
            .unwrap_or_else(|| original_model.to_string())
    }
}

/// Normalize a base_url by ensuring it has a trailing slash.
pub fn normalize_base_url(url: &str) -> String {
    let mut s = url.trim().to_string();
    if s.is_empty() {
        return s;
    }
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// Resolve a ServiceProvider into a ResolvedProvider (validate base_url + api_key).
fn resolve_service_provider(sp: &ServiceProvider) -> Option<ResolvedProvider> {
    let (base_url, family_id) = resolve_upstream(sp.base_url.clone(), sp.endpoint_id.as_deref())?;
    let base_url = normalize_base_url(&base_url);
    let api_key = sp.api_key.clone()?;
    if api_key.is_empty() {
        return None;
    }
    Some(ResolvedProvider {
        name: sp.name.clone(),
        provider_type: sp.provider_type.clone(),
        endpoint_id: sp.endpoint_id.clone(),
        base_url,
        api_key,
        family_id,
        default_model: sp.default_model.clone(),
        model_mapping: sp.model_mapping.clone(),
    })
}

/// Resolves upstream base_url and optional family_id.
///
/// Priority:
/// 1. If `endpoint_id` is provided and recognized by `llm_providers`, use its `base_url`.
/// 2. Otherwise, use `provider_base_url` (if it's not empty).
pub fn resolve_upstream(
    provider_base_url: Option<String>,
    endpoint_id: Option<&str>,
) -> Option<(String, Option<String>)> {
    if let Some(eid) = endpoint_id {
        let eid = eid.trim();
        if !eid.is_empty() {
            if let Some((family_id, endpoint)) = get_endpoint(eid) {
                // Priority 1: Use llm_providers data as single source of truth
                return Some((
                    normalize_base_url(endpoint.base_url),
                    Some(family_id.to_string()),
                ));
            }
            tracing::debug!(
                "get_endpoint({:?}) returned None, falling back to provider base_url",
                eid
            );
        }
    }

    // Priority 2: User-provided base_url (or custom Provider not in registry)
    let url = provider_base_url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    Some((normalize_base_url(url), None))
}

/// Extract target provider hint from request headers or body.
pub fn target_provider_hint(headers: &HeaderMap, payload: &Value) -> Option<String> {
    let from_header = headers
        .get("x-unigateway-provider")
        .or_else(|| headers.get("x-target-vendor"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    if from_header.is_some() {
        return from_header;
    }
    payload
        .get("target_vendor")
        .or_else(|| payload.get("target_provider"))
        .or_else(|| payload.get("provider"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

/// Resolve providers to try for a request.
///
/// - `target_hint` set: returns exactly the matching provider.
/// - `routing_strategy == "fallback"`: returns all providers sorted by binding priority.
/// - otherwise (round-robin): returns a single provider chosen by round-robin.
///
/// Each returned provider has its upstream base_url and api_key validated.
/// Returns `Err(message)` on failure.
pub async fn resolve_providers(
    gateway: &GatewayState,
    service_id: &str,
    protocol_hint: &str, // The protocol requested by the client
    target_hint: Option<&str>,
) -> Result<Vec<ResolvedProvider>, String> {
    if let Some(hint) = target_hint {
        let sp = if let Some(sp) = gateway
            .select_provider_for_service_with_hint(service_id, "", hint)
            .await
        {
            sp
        } else {
            gateway
                .select_provider_for_service_with_hint(service_id, protocol_hint, hint)
                .await
                .ok_or_else(|| format!("no provider matches target '{hint}'"))?
        };
        let rp = resolve_service_provider(&sp)
            .ok_or_else(|| format!("provider '{}': base_url or api_key missing", sp.name))?;
        return Ok(vec![rp]);
    }

    let strategy = gateway.get_routing_strategy(service_id).await;

    if strategy == "fallback" {
        let mut all = gateway
            .select_all_providers_for_service(service_id, "")
            .await;

        if all.is_empty() {
            all = gateway
                .select_all_providers_for_service(service_id, protocol_hint)
                .await;
        }

        if all.is_empty() {
            return Err(format!("no provider bound for service/{protocol_hint}"));
        }
        let resolved: Vec<ResolvedProvider> =
            all.iter().filter_map(resolve_service_provider).collect();
        if resolved.is_empty() {
            return Err("all bound providers have missing base_url or api_key".to_string());
        }
        return Ok(resolved);
    }

    // round-robin
    let mut sp = gateway
        .select_provider_for_service(service_id, "")
        .await;

    if sp.is_none() {
        sp = gateway
            .select_provider_for_service(service_id, protocol_hint)
            .await;
    }

    let sp = sp.ok_or_else(|| format!("no provider bound for service/{protocol_hint}"))?;
    let rp = resolve_service_provider(&sp)
        .ok_or_else(|| format!("provider '{}': base_url or api_key missing", sp.name))?;
    Ok(vec![rp])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{resolve_providers, resolve_upstream};
    use crate::config::GatewayState;
    use tempfile::tempdir;

    #[test]
    fn resolve_upstream_minimax_global() {
        let r = resolve_upstream(None, Some("minimax:global"));
        let (url, family) = r.expect("get_endpoint(minimax:global) should return Some");
        assert!(
            url.contains("minimax"),
            "base_url should contain minimax: {}",
            url
        );
        assert_eq!(family.as_deref(), Some("minimax"));
    }

    #[tokio::test]
    async fn resolve_providers_allows_cross_protocol_target_hint() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state: Arc<GatewayState> = GatewayState::load(&config_path).await.expect("load state");

        state.create_service("svc", "Service").await;
        let provider_id = state
            .create_provider(
                "moonshot",
                "openai",
                "moonshot:global",
                None,
                "sk-test-moonshot",
                None,
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let providers = resolve_providers(&state, "svc", "anthropic", Some("moonshot"))
            .await
            .expect("resolve providers");

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "moonshot");
        assert_eq!(providers[0].provider_type, "openai");
    }

    #[tokio::test]
    async fn resolve_providers_allows_cross_protocol_round_robin() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state: Arc<GatewayState> = GatewayState::load(&config_path).await.expect("load state");

        state.create_service("svc", "Service").await;
        let provider_id = state
            .create_provider(
                "moonshot",
                "openai",
                "moonshot:global",
                None,
                "sk-test-moonshot",
                None,
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let providers = resolve_providers(&state, "svc", "anthropic", None)
            .await
            .expect("resolve providers");

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "moonshot");
        assert_eq!(providers[0].provider_type, "openai");
    }
}
