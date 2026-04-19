use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GatewayConfigFile {
    #[serde(default)]
    pub preferences: GatewayPreferences,
    pub services: Vec<ServiceEntry>,
    pub providers: Vec<ProviderEntry>,
    pub bindings: Vec<BindingEntry>,
    pub api_keys: Vec<ApiKeyEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GatewayPreferences {
    #[serde(default)]
    pub default_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    pub id: String,
    pub name: String,
    #[serde(default = "default_round_robin")]
    pub routing_strategy: String,
}

pub(super) fn default_round_robin() -> String {
    "round_robin".to_string()
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
    pub default_model: String,
    #[serde(default)]
    pub model_mapping: String,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

fn default_true() -> bool {
    true
}

pub struct ProviderModelOptions<'a> {
    pub default_model: Option<&'a str>,
    pub model_mapping: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingEntry {
    pub service_id: String,
    pub provider_name: String,
    #[serde(default)]
    pub priority: i64,
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

#[derive(Debug, Clone, Serialize)]
pub struct ModeProvider {
    pub name: String,
    pub provider_type: String,
    pub endpoint_id: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub model_mapping: Option<String>,
    pub has_api_key: bool,
    pub is_enabled: bool,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModeKey {
    pub key: String,
    pub is_active: bool,
    pub quota_limit: Option<i64>,
    pub qps_limit: Option<f64>,
    pub concurrency_limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModeView {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub routing_strategy: String,
    pub providers: Vec<ModeProvider>,
    pub keys: Vec<ModeKey>,
}

pub fn build_mode_views(file: &GatewayConfigFile, default_mode: &str) -> Vec<ModeView> {
    let mut modes = Vec::new();
    for service in &file.services {
        let mut providers: Vec<ModeProvider> = file
            .bindings
            .iter()
            .filter(|binding| binding.service_id == service.id)
            .map(|binding| {
                let provider = file
                    .providers
                    .iter()
                    .find(|provider| provider.name == binding.provider_name);
                ModeProvider {
                    name: binding.provider_name.clone(),
                    provider_type: provider
                        .map(|provider| provider.provider_type.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    endpoint_id: provider.and_then(|provider| {
                        if provider.endpoint_id.is_empty() {
                            None
                        } else {
                            Some(provider.endpoint_id.clone())
                        }
                    }),
                    base_url: provider.and_then(|provider| {
                        if provider.base_url.is_empty() {
                            None
                        } else {
                            Some(provider.base_url.clone())
                        }
                    }),
                    default_model: provider.and_then(|provider| {
                        if provider.default_model.is_empty() {
                            None
                        } else {
                            Some(provider.default_model.clone())
                        }
                    }),
                    model_mapping: provider.and_then(|provider| {
                        if provider.model_mapping.is_empty() {
                            None
                        } else {
                            Some(provider.model_mapping.clone())
                        }
                    }),
                    has_api_key: provider
                        .map(|provider| !provider.api_key.is_empty())
                        .unwrap_or(false),
                    is_enabled: provider
                        .map(|provider| provider.is_enabled)
                        .unwrap_or(false),
                    priority: binding.priority,
                }
            })
            .collect();
        providers.sort_by_key(|provider| provider.priority);

        let keys = file
            .api_keys
            .iter()
            .filter(|key| key.service_id == service.id)
            .map(|key| ModeKey {
                key: key.key.clone(),
                is_active: key.is_active,
                quota_limit: key.quota_limit,
                qps_limit: key.qps_limit,
                concurrency_limit: key.concurrency_limit,
            })
            .collect();

        modes.push(ModeView {
            id: service.id.clone(),
            name: service.name.clone(),
            is_default: !default_mode.is_empty() && default_mode == service.id,
            routing_strategy: service.routing_strategy.clone(),
            providers,
            keys,
        });
    }

    modes
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
    pub default_model: Option<String>,
    pub model_mapping: Option<String>,
}
