use std::{collections::HashMap, sync::Arc, time::Instant};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::app::AppConfig;

#[derive(Clone)]
pub(crate) struct AppState {
    pub pool: SqlitePool,
    pub config: AppConfig,
    pub api_key_runtime: Arc<Mutex<HashMap<String, RuntimeRateState>>>,
    pub service_rr: Arc<Mutex<HashMap<String, usize>>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct GatewayApiKey {
    pub key: String,
    pub service_id: String,
    pub quota_limit: Option<i64>,
    pub used_quota: i64,
    pub is_active: i64,
    pub qps_limit: Option<f64>,
    pub concurrency_limit: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct ServiceProvider {
    pub name: String,
    pub provider_type: String,
    pub endpoint_id: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_mapping: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RuntimeRateState {
    pub window_started_at: Instant,
    pub request_count: u64,
    pub in_flight: u64,
}

#[derive(Deserialize)]
pub(crate) struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub(crate) struct ModelList {
    pub object: &'static str,
    pub data: Vec<ModelItem>,
}

#[derive(Serialize)]
pub(crate) struct ModelItem {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub owned_by: &'static str,
}
