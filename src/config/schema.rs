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
