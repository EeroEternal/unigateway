use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    routing::{delete, get, post},
    Router,
};
use sqlx::sqlite::SqlitePoolOptions;
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::trace::TraceLayer;
use tracing::info;

#[path = "app/admin.rs"]
mod admin;
#[path = "app/auth.rs"]
mod auth;
#[path = "app/gateway.rs"]
mod gateway;
#[path = "app/storage.rs"]
mod storage;
#[path = "app/types.rs"]
mod types;

use types::AppState;

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
        Self {
            bind: std::env::var("UNIGATEWAY_BIND").unwrap_or_else(|_| "127.0.0.1:3210".to_string()),
            db_url: std::env::var("UNIGATEWAY_DB")
                .unwrap_or_else(|_| "sqlite://unigateway.db".to_string()),
            enable_ui: std::env::var("UNIGATEWAY_ENABLE_UI")
                .map(|v| v != "0" && v.to_lowercase() != "false")
                .unwrap_or(true),
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

pub async fn run(config: AppConfig) -> Result<()> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.db_url)
        .await
        .with_context(|| format!("failed to connect sqlite: {}", config.db_url))?;

    storage::init_db(&pool).await?;

    let state = AppState {
        pool,
        config: config.clone(),
        api_key_runtime: Arc::new(Mutex::new(HashMap::new())),
        service_rr: Arc::new(Mutex::new(HashMap::new())),
    };

    let mut app = Router::new()
        .route("/health", get(admin::health))
        .route("/metrics", get(admin::metrics))
        .route("/v1/models", get(admin::models))
        .route(
            "/api/admin/services",
            get(admin::api_list_services).post(admin::api_create_service),
        )
        .route(
            "/api/admin/providers",
            get(admin::api_list_providers).post(admin::api_create_provider),
        )
        .route("/api/admin/bindings", post(admin::api_bind_provider))
        .route(
            "/api/admin/api-keys",
            get(admin::api_list_api_keys).post(admin::api_create_api_key),
        )
        .route("/v1/chat/completions", post(gateway::openai_chat))
        .route("/v1/messages", post(gateway::anthropic_messages));

    if config.enable_ui {
        app = app
            .route("/", get(admin::home))
            .route("/login", get(auth::login_page).post(auth::login))
            .route("/logout", post(auth::logout))
            .route("/admin", get(admin::admin_page))
            .route("/admin/dashboard", get(admin::admin_dashboard))
            .route("/admin/stats", get(admin::admin_stats_partial))
            .route("/admin/providers", get(admin::admin_providers))
            .route(
                "/admin/providers/list",
                get(admin::admin_providers_list_partial),
            )
            .route(
                "/admin/providers/create",
                post(admin::admin_create_provider_partial),
            )
            .route(
                "/admin/providers/:id",
                delete(admin::admin_providers_delete),
            )
            .route("/admin/api-keys", get(admin::admin_api_keys_page))
            .route(
                "/admin/api-keys/list",
                get(admin::admin_api_keys_list_partial),
            )
            .route(
                "/admin/api-keys/create",
                post(admin::admin_create_api_key_partial),
            )
            .route("/admin/api-keys/:id", delete(admin::admin_api_keys_delete))
            .route("/admin/logs", get(admin::admin_logs_page))
            .route("/admin/logs/list", get(admin::admin_logs_list_partial))
            .route("/admin/settings", get(admin::admin_settings_page));
    }

    let app = app
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

pub use storage::hash_password;
