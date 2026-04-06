use std::sync::Arc;

use serde::Serialize;
use unigateway_core::InMemoryDriverRegistry;
use unigateway_core::UniGatewayEngine;
use unigateway_core::protocol::builtin_drivers;
use unigateway_core::transport::ReqwestHttpTransport;

use crate::config::GatewayState;
use crate::config::core_sync::sync_core_pools;

pub fn default_config_path() -> String {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unigateway");
    dir.join("config.toml").to_string_lossy().into_owned()
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: String,
    pub config_path: String,
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
            config_path: std::env::var("UNIGATEWAY_CONFIG")
                .unwrap_or_else(|_| default_config_path()),
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
    pub config: AppConfig,
    pub gateway: Arc<GatewayState>,
    pub core_engine: Arc<UniGatewayEngine>,
}

impl AppState {
    pub fn new(config: AppConfig, gateway: Arc<GatewayState>) -> Self {
        let core_driver_registry = Arc::new(InMemoryDriverRegistry::new());
        for driver in builtin_drivers(Arc::new(ReqwestHttpTransport::default())) {
            core_driver_registry.register(driver);
        }
        let core_engine = Arc::new(
            UniGatewayEngine::builder()
                .with_driver_registry(core_driver_registry.clone())
                .build(),
        );

        Self {
            config,
            gateway,
            core_engine,
        }
    }

    pub async fn sync_core_pools(&self) -> anyhow::Result<()> {
        sync_core_pools(self.gateway.as_ref(), self.core_engine.as_ref()).await
    }
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
