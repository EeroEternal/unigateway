use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::retry::{LoadBalancingStrategy, RetryPolicy};

pub type PoolId = String;
pub type EndpointId = String;
pub type DriverId = String;
pub type RequestId = String;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString([REDACTED])")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    OpenAiCompatible,
    Anthropic,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPolicy {
    pub default_model: Option<String>,
    pub model_mapping: HashMap<String, String>,
}

/// Provider endpoint backing a pool entry.
///
/// # Hint Matching
///
/// Host-side provider hints are matched case-insensitively against `endpoint_id`,
/// `provider_name`, `source_endpoint_id`, and `provider_family`.
///
/// For stable embedder behavior, prefer keeping `provider_name`, `source_endpoint_id`, and
/// `provider_family` populated and stable across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Stable endpoint identifier used by engine routing and direct endpoint selection.
    pub endpoint_id: EndpointId,
    /// Human-facing provider name used for hint matching, for example `deepseek-main`.
    pub provider_name: Option<String>,
    /// Original upstream/source endpoint identifier from the config or embedder domain.
    pub source_endpoint_id: Option<String>,
    /// Provider family or vendor grouping used by higher-level hint matching, for example `deepseek`.
    pub provider_family: Option<String>,
    pub provider_kind: ProviderKind,
    pub driver_id: DriverId,
    pub base_url: String,
    pub api_key: SecretString,
    pub model_policy: ModelPolicy,
    pub enabled: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPool {
    pub pool_id: PoolId,
    pub endpoints: Vec<Endpoint>,
    pub load_balancing: LoadBalancingStrategy,
    pub retry_policy: RetryPolicy,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSummary {
    pub pool_id: PoolId,
    pub endpoint_count: usize,
    pub load_balancing: LoadBalancingStrategy,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointRef {
    pub endpoint_id: EndpointId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionTarget {
    Pool { pool_id: PoolId },
    Plan(ExecutionPlan),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub pool_id: Option<PoolId>,
    pub candidates: Vec<EndpointRef>,
    pub load_balancing_override: Option<LoadBalancingStrategy>,
    pub retry_policy_override: Option<RetryPolicy>,
    pub metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::SecretString;

    #[test]
    fn secret_string_debug_is_redacted() {
        let value = SecretString::new("secret-token");
        assert_eq!(format!("{value:?}"), "SecretString([REDACTED])");
    }
}
