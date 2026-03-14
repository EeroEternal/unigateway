use std::{collections::HashMap, net::SocketAddr, path::Path, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, post},
};
use sqlx::sqlite::SqlitePoolOptions;
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::types::{AppConfig, AppState};

pub async fn run(config: AppConfig) -> Result<()> {
    if config.db_url.starts_with("sqlite://") {
        let db_path = config.db_url.strip_prefix("sqlite://").unwrap();
        if !Path::new(db_path).exists() {
            info!("Creating database file: {}", db_path);
            std::fs::File::create(db_path)
                .with_context(|| format!("failed to create database file: {}", db_path))?;
        }
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.db_url)
        .await
        .with_context(|| format!("failed to connect sqlite: {}", config.db_url))?;

    crate::storage::init_db(&pool).await?;

    let state = AppState {
        pool,
        config: config.clone(),
        api_key_runtime: Arc::new(Mutex::new(HashMap::new())),
        service_rr: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(crate::system::health))
        .route("/metrics", get(crate::system::metrics))
        .route("/v1/models", get(crate::system::models))
        .route(
            "/api/admin/services",
            get(crate::service::api_list_services).post(crate::service::api_create_service),
        )
        .route(
            "/api/admin/providers",
            get(crate::provider::api_list_providers).post(crate::provider::api_create_provider),
        )
        .route("/api/admin/bindings", post(crate::provider::api_bind_provider))
        .route(
            "/api/admin/api-keys",
            get(crate::api_key::api_list_api_keys).post(crate::api_key::api_create_api_key),
        )
        .route("/v1/chat/completions", post(crate::gateway::openai_chat))
        .route("/v1/messages", post(crate::gateway::anthropic_messages))
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

pub use crate::storage::hash_password;
