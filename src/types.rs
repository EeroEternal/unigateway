use std::{collections::HashMap, sync::Arc, time::Instant};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: String,
    pub db_url: String,
    pub enable_ui: bool,
    pub admin_token: String,
    pub openai_base_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub anthropic_base_url: String,
    pub anthropic_api_key: String,
    pub anthropic_model: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let bind = std::env::var("UNIGATEWAY_BIND").unwrap_or_else(|_| {
            std::env::var("PORT")
                .map(|port| format!("0.0.0.0:{port}"))
                .unwrap_or_else(|_| "127.0.0.1:3210".to_string())
        });

        Self {
            bind,
            db_url: std::env::var("UNIGATEWAY_DB")
                .unwrap_or_else(|_| "sqlite://unigateway.db".to_string()),
            enable_ui: std::env::var("UNIGATEWAY_ENABLE_UI")
                .map(|v| v != "0" && v.to_lowercase() != "false")
                .unwrap_or(false),
            admin_token: std::env::var("UNIGATEWAY_ADMIN_TOKEN").unwrap_or_default(),
            openai_base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
            openai_api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            openai_model: std::env::var("OPENAI_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            anthropic_base_url: std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            anthropic_model: std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-latest".to_string()),
        }
    }
}

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
    #[allow(dead_code)]
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
