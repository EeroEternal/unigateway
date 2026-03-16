use super::{GatewayApiKey, GatewayState, ProviderEntry, ServiceProvider, default_round_robin};

fn to_service_provider(p: ProviderEntry) -> ServiceProvider {
    ServiceProvider {
        name: p.name,
        provider_type: p.provider_type,
        endpoint_id: if p.endpoint_id.is_empty() {
            None
        } else {
            Some(p.endpoint_id)
        },
        base_url: if p.base_url.is_empty() {
            None
        } else {
            Some(p.base_url)
        },
        api_key: Some(p.api_key),
        default_model: if p.default_model.is_empty() {
            None
        } else {
            Some(p.default_model)
        },
        model_mapping: if p.model_mapping.is_empty() {
            None
        } else {
            Some(p.model_mapping)
        },
    }
}

impl GatewayState {
    pub async fn find_gateway_api_key(&self, raw_key: &str) -> Option<GatewayApiKey> {
        let guard = self.inner.read().await;
        let k = guard.file.api_keys.iter().find(|a| a.key == raw_key)?;
        Some(GatewayApiKey {
            key: k.key.clone(),
            service_id: k.service_id.clone(),
            quota_limit: k.quota_limit,
            used_quota: k.used_quota,
            is_active: if k.is_active { 1 } else { 0 },
            qps_limit: k.qps_limit,
            concurrency_limit: k.concurrency_limit,
        })
    }

    pub async fn get_routing_strategy(&self, service_id: &str) -> String {
        let guard = self.inner.read().await;
        guard
            .file
            .services
            .iter()
            .find(|s| s.id == service_id)
            .map(|s| s.routing_strategy.clone())
            .unwrap_or_else(default_round_robin)
    }

    pub async fn select_all_providers_for_service(
        &self,
        service_id: &str,
        protocol: &str,
    ) -> Vec<ServiceProvider> {
        let guard = self.inner.read().await;
        let mut binding_priorities: Vec<(String, i64)> = guard
            .file
            .bindings
            .iter()
            .filter(|b| b.service_id == service_id)
            .map(|b| (b.provider_name.clone(), b.priority))
            .collect();
        binding_priorities.sort_by_key(|(_, prio)| *prio);

        let mut result = Vec::new();
        for (name, _) in &binding_priorities {
            if let Some(p) = guard
                .file
                .providers
                .iter()
                .find(|p| {
                    p.is_enabled
                        && (protocol.is_empty() || p.provider_type == protocol)
                        && p.name == *name
                })
                .cloned()
            {
                result.push(to_service_provider(p));
            }
        }
        result
    }

    pub async fn select_provider_for_service(
        &self,
        service_id: &str,
        protocol: &str,
    ) -> Option<ServiceProvider> {
        let providers: Vec<ProviderEntry> = {
            let guard = self.inner.read().await;
            let names: Vec<String> = guard
                .file
                .bindings
                .iter()
                .filter(|b| b.service_id == service_id)
                .map(|b| b.provider_name.clone())
                .collect();
            guard
                .file
                .providers
                .iter()
                .filter(|p| {
                    p.is_enabled
                        && (protocol.is_empty() || p.provider_type == protocol)
                        && names.contains(&p.name)
                })
                .cloned()
                .collect()
        };
        if providers.is_empty() {
            return None;
        }
        let bucket = format!("{}:{}", service_id, protocol);
        let mut rr = self.service_rr.lock().await;
        let idx = rr.entry(bucket).or_insert(0);
        let p = providers[*idx % providers.len()].clone();
        *idx = (*idx + 1) % providers.len();
        Some(to_service_provider(p))
    }

    pub async fn select_provider_for_service_with_hint(
        &self,
        service_id: &str,
        protocol: &str,
        hint: &str,
    ) -> Option<ServiceProvider> {
        let needle = hint.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.select_provider_for_service(service_id, protocol).await;
        }

        let providers: Vec<ProviderEntry> = {
            let guard = self.inner.read().await;
            let names: Vec<String> = guard
                .file
                .bindings
                .iter()
                .filter(|b| b.service_id == service_id)
                .map(|b| b.provider_name.clone())
                .collect();
            guard
                .file
                .providers
                .iter()
                .filter(|p| {
                    p.is_enabled
                        && (protocol.is_empty() || p.provider_type == protocol)
                        && names.contains(&p.name)
                })
                .cloned()
                .collect()
        };

        let p = providers.into_iter().find(|p| {
            if p.name.eq_ignore_ascii_case(&needle) {
                return true;
            }
            if p.endpoint_id.eq_ignore_ascii_case(&needle) {
                return true;
            }
            p.endpoint_id
                .split(':')
                .next()
                .map(|family| family.eq_ignore_ascii_case(&needle))
                .unwrap_or(false)
        })?;

        Some(to_service_provider(p))
    }

    pub async fn increment_used_quota(&self, key: &str) {
        let mut guard = self.inner.write().await;
        if let Some(k) = guard.file.api_keys.iter_mut().find(|a| a.key == key) {
            k.used_quota += 1;
            guard.dirty = true;
        }
    }

    pub async fn record_stat(&self, endpoint: &str, _status_code: u16, _latency_ms: i64) {
        let mut guard = self.inner.write().await;
        guard.request_stats.total += 1;
        if endpoint == "/v1/chat/completions" {
            guard.request_stats.openai_total += 1;
        } else if endpoint == "/v1/messages" {
            guard.request_stats.anthropic_total += 1;
        } else if endpoint == "/v1/embeddings" {
            guard.request_stats.embeddings_total += 1;
        }
    }

    pub async fn metrics_snapshot(&self) -> (u64, u64, u64, u64) {
        let guard = self.inner.read().await;
        (
            guard.request_stats.total,
            guard.request_stats.openai_total,
            guard.request_stats.anthropic_total,
            guard.request_stats.embeddings_total,
        )
    }
}
