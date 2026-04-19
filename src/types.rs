use std::sync::Arc;

use serde::Serialize;
use unigateway_core::UniGatewayEngine;
use unigateway_host::host::HostEnvProvider;

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

#[derive(Clone)]
pub(crate) struct GatewayRequestState {
    bind: String,
    openai_base_url: String,
    openai_api_key: String,
    openai_model: String,
    anthropic_base_url: String,
    anthropic_api_key: String,
    anthropic_model: String,
    gateway: Arc<GatewayState>,
    core_engine: Arc<UniGatewayEngine>,
}

#[derive(Clone)]
pub(crate) struct SystemState {
    openai_model: String,
    anthropic_model: String,
    gateway: Arc<GatewayState>,
}

impl AppState {
    pub fn new(config: AppConfig, gateway: Arc<GatewayState>) -> Self {
        let core_engine = Arc::new(
            UniGatewayEngine::builder()
                .with_builtin_http_drivers()
                .with_hooks(Arc::new(crate::telemetry::GatewayTelemetryHooks))
                .build()
                .expect("Failed to initialize core engine"),
        );

        Self {
            config,
            gateway,
            core_engine,
        }
    }

    pub fn admin_token(&self) -> &str {
        self.config.admin_token.as_str()
    }

    pub fn engine(&self) -> &UniGatewayEngine {
        self.core_engine.as_ref()
    }

    pub async fn sync_core_pools(&self) -> anyhow::Result<()> {
        sync_core_pools(self.gateway.as_ref(), self.engine()).await
    }
}

impl GatewayRequestState {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            bind: state.config.bind.clone(),
            openai_base_url: state.config.openai_base_url.clone(),
            openai_api_key: state.config.openai_api_key.clone(),
            openai_model: state.config.openai_model.clone(),
            anthropic_base_url: state.config.anthropic_base_url.clone(),
            anthropic_api_key: state.config.anthropic_api_key.clone(),
            anthropic_model: state.config.anthropic_model.clone(),
            gateway: state.gateway.clone(),
            core_engine: state.core_engine.clone(),
        }
    }

    pub fn gateway(&self) -> &GatewayState {
        self.gateway.as_ref()
    }

    pub fn engine(&self) -> &UniGatewayEngine {
        self.core_engine.as_ref()
    }

    pub fn is_local_bind(&self) -> bool {
        self.bind.starts_with("127.0.0.1") || self.bind.starts_with("localhost")
    }

    pub fn provider_base_url(&self, provider: HostEnvProvider) -> &str {
        match provider {
            HostEnvProvider::OpenAi => self.openai_base_url.as_str(),
            HostEnvProvider::Anthropic => self.anthropic_base_url.as_str(),
        }
    }

    pub fn provider_api_key(&self, provider: HostEnvProvider) -> &str {
        match provider {
            HostEnvProvider::OpenAi => self.openai_api_key.as_str(),
            HostEnvProvider::Anthropic => self.anthropic_api_key.as_str(),
        }
    }

    pub fn provider_model(&self, provider: HostEnvProvider) -> &str {
        match provider {
            HostEnvProvider::OpenAi => self.openai_model.as_str(),
            HostEnvProvider::Anthropic => self.anthropic_model.as_str(),
        }
    }
}

impl SystemState {
    pub fn from_app_state(state: &AppState) -> Self {
        Self {
            openai_model: state.config.openai_model.clone(),
            anthropic_model: state.config.anthropic_model.clone(),
            gateway: state.gateway.clone(),
        }
    }

    pub fn gateway(&self) -> &GatewayState {
        self.gateway.as_ref()
    }

    pub fn provider_model(&self, provider: HostEnvProvider) -> &str {
        match provider {
            HostEnvProvider::OpenAi => self.openai_model.as_str(),
            HostEnvProvider::Anthropic => self.anthropic_model.as_str(),
        }
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
