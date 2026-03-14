//! Config file + in-memory state: load from TOML, mutate in memory, persist back to file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

/// What we persist to TOML (and load from).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GatewayConfigFile {
    pub services: Vec<ServiceEntry>,
    pub providers: Vec<ProviderEntry>,
    /// (service_id, provider_name)
    pub bindings: Vec<BindingEntry>,
    pub api_keys: Vec<ApiKeyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub name: String,
    pub provider_type: String,
    pub endpoint_id: String,
    #[serde(default)]
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub model_mapping: String,
    #[serde(default = "one")]
    pub weight: i64,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

fn one() -> i64 {
    1
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingEntry {
    pub service_id: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub key: String,
    pub service_id: String,
    #[serde(default)]
    pub quota_limit: Option<i64>,
    #[serde(default)]
    pub used_quota: i64,
    #[serde(default = "default_true")]
    pub is_active: bool,
    #[serde(default)]
    pub qps_limit: Option<f64>,
    #[serde(default)]
    pub concurrency_limit: Option<i64>,
}

impl Default for ApiKeyEntry {
    fn default() -> Self {
        Self {
            key: String::new(),
            service_id: String::new(),
            quota_limit: None,
            used_quota: 0,
            is_active: true,
            qps_limit: None,
            concurrency_limit: None,
        }
    }
}

/// In-memory state: config data + request counters (not persisted).
#[derive(Debug, Clone)]
pub struct RequestStats {
    pub total: u64,
    pub openai_total: u64,
    pub anthropic_total: u64,
}

/// Full in-memory gateway state: file data + stats. Persist writes only file data.
#[derive(Debug)]
pub struct GatewayConfig {
    pub file: GatewayConfigFile,
    pub request_stats: RequestStats,
    /// When true, next persist() will write (e.g. used_quota changed).
    pub dirty: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            file: GatewayConfigFile::default(),
            request_stats: RequestStats {
                total: 0,
                openai_total: 0,
                anthropic_total: 0,
            },
            dirty: false,
        }
    }
}

/// Shared gateway state: config (with lock) + runtime rate state.
pub struct GatewayState {
    pub config_path: std::path::PathBuf,
    pub inner: RwLock<GatewayConfig>,
    pub api_key_runtime: Mutex<HashMap<String, RuntimeRateState>>,
    pub service_rr: Mutex<HashMap<String, usize>>,
}

#[derive(Debug, Clone)]
pub struct RuntimeRateState {
    pub window_started_at: Instant,
    pub request_count: u64,
    pub in_flight: u64,
}

/// Same shape as before for gateway/authz.
#[derive(Debug, Clone)]
pub struct GatewayApiKey {
    pub key: String,
    pub service_id: String,
    pub quota_limit: Option<i64>,
    pub used_quota: i64,
    pub is_active: i64,
    pub qps_limit: Option<f64>,
    pub concurrency_limit: Option<i64>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ServiceProvider {
    pub name: String,
    pub provider_type: String,
    pub endpoint_id: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_mapping: Option<String>,
}

impl GatewayState {
    pub async fn load(config_path: &Path) -> Result<Arc<Self>> {
        let path = config_path.to_path_buf();
        let file = if path.exists() {
            let s = tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("read config: {}", path.display()))?;
            toml::from_str::<GatewayConfigFile>(&s).context("parse config TOML")?
        } else {
            GatewayConfigFile::default()
        };
        let config = GatewayConfig {
            file,
            request_stats: RequestStats {
                total: 0,
                openai_total: 0,
                anthropic_total: 0,
            },
            dirty: false,
        };
        Ok(Arc::new(Self {
            config_path: path,
            inner: RwLock::new(config),
            api_key_runtime: Mutex::new(HashMap::new()),
            service_rr: Mutex::new(HashMap::new()),
        }))
    }

    /// Atomic write: temp file then rename.
    pub async fn persist(&self) -> Result<()> {
        let to_write = {
            let guard = self.inner.read().await;
            if !guard.dirty {
                return Ok(());
            }
            guard.file.clone()
        };
        let s = toml::to_string_pretty(&to_write).context("serialize config")?;
        let tmp = self.config_path.with_extension("tmp");
        tokio::fs::write(&tmp, s)
            .await
            .with_context(|| format!("write config: {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &self.config_path)
            .await
            .with_context(|| format!("rename config: {}", self.config_path.display()))?;
        self.inner.write().await.dirty = false;
        Ok(())
    }

    /// Persist if dirty (e.g. after structural change or used_quota update). Call from CLI or admin API.
    pub async fn persist_if_dirty(&self) -> Result<()> {
        if self.inner.read().await.dirty {
            self.persist().await
        } else {
            Ok(())
        }
    }

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

    pub async fn select_provider_for_service(
        &self,
        service_id: &str,
        protocol: &str,
    ) -> Option<ServiceProvider> {
        let (_provider_names, providers): (Vec<String>, Vec<ProviderEntry>) = {
            let guard = self.inner.read().await;
            let names: Vec<String> = guard
                .file
                .bindings
                .iter()
                .filter(|b| b.service_id == service_id)
                .map(|b| b.provider_name.clone())
                .collect();
            let list: Vec<ProviderEntry> = guard
                .file
                .providers
                .iter()
                .filter(|p| p.is_enabled && p.provider_type == protocol && names.contains(&p.name))
                .cloned()
                .collect();
            (names, list)
        };
        if providers.is_empty() {
            return None;
        }
        let bucket = format!("{}:{}", service_id, protocol);
        let mut rr = self.service_rr.lock().await;
        let idx = rr.entry(bucket).or_insert(0);
        let p = providers[*idx % providers.len()].clone();
        *idx = (*idx + 1) % providers.len();
        Some(ServiceProvider {
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
            model_mapping: if p.model_mapping.is_empty() {
                None
            } else {
                Some(p.model_mapping)
            },
        })
    }

    /// Select a specific bound provider by name/family hint.
    /// - name match: provider.name
    /// - family match: endpoint_id prefix before ":" (e.g. "minimax:global" -> "minimax")
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
                .filter(|p| p.is_enabled && p.provider_type == protocol && names.contains(&p.name))
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

        Some(ServiceProvider {
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
            model_mapping: if p.model_mapping.is_empty() {
                None
            } else {
                Some(p.model_mapping)
            },
        })
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
        }
        // Optionally persist used_quota periodically; we don't persist request_stats.
    }

    pub async fn metrics_snapshot(&self) -> (u64, u64, u64) {
        let guard = self.inner.read().await;
        (
            guard.request_stats.total,
            guard.request_stats.openai_total,
            guard.request_stats.anthropic_total,
        )
    }

    // --- Admin: list/create/bind (used by HTTP API and CLI) ---

    pub async fn list_services(&self) -> Vec<(String, String)> {
        let guard = self.inner.read().await;
        guard.file.services.iter().map(|s| (s.id.clone(), s.name.clone())).collect()
    }

    pub async fn create_service(&self, id: &str, name: &str) {
        let mut guard = self.inner.write().await;
        if let Some(s) = guard.file.services.iter_mut().find(|s| s.id == id) {
            s.name = name.to_string();
        } else {
            guard.file.services.push(ServiceEntry { id: id.to_string(), name: name.to_string() });
        }
        guard.dirty = true;
    }

    pub async fn list_providers(&self) -> Vec<(i64, String, String, Option<String>, Option<String>)> {
        let guard = self.inner.read().await;
        guard.file.providers.iter().enumerate().map(|(i, p)| {
            (i as i64, p.name.clone(), p.provider_type.clone(),
             if p.endpoint_id.is_empty() { None } else { Some(p.endpoint_id.clone()) },
             if p.base_url.is_empty() { None } else { Some(p.base_url.clone()) })
        }).collect()
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
        let mut guard = self.inner.write().await;
        let idx = guard.file.providers.len() as i64;
        guard.file.providers.push(ProviderEntry {
            name: name.to_string(),
            provider_type: provider_type.to_string(),
            endpoint_id: endpoint_id.to_string(),
            base_url: base_url.unwrap_or("").to_string(),
            api_key: api_key.to_string(),
            model_mapping: model_mapping.unwrap_or("").to_string(),
            weight: 1,
            is_enabled: true,
        });
        guard.dirty = true;
        idx
    }

    pub async fn bind_provider_to_service(&self, service_id: &str, provider_id: i64) -> Result<()> {
        let provider_name = {
            let guard = self.inner.read().await;
            let idx = provider_id as usize;
            guard.file.providers.get(idx).map(|p| p.name.clone())
        };
        let Some(provider_name) = provider_name else {
            anyhow::bail!("provider_id {} not found", provider_id);
        };
        let mut guard = self.inner.write().await;
        let exists = guard.file.bindings.iter().any(|b| b.service_id == service_id && b.provider_name == provider_name);
        if !exists {
            guard.file.bindings.push(BindingEntry { service_id: service_id.to_string(), provider_name });
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
        let used = guard.file.api_keys.iter().find(|a| a.key == key).map(|a| a.used_quota).unwrap_or(0);
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
}
