use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::GatewayState;
use crate::types::{AppConfig, AppState};

pub async fn run(config: AppConfig) -> Result<()> {
    let config_path = std::path::Path::new(&config.config_path);
    let gateway = GatewayState::load(config_path)
        .await
        .with_context(|| format!("load config: {}", config.config_path))?;
    let state = AppState {
        config: config.clone(),
        gateway: gateway.clone(),
    };

    // Periodically persist used_quota and other dirty state to config file
    let gw = gateway.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let _ = gw.persist_if_dirty().await;
        }
    });

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
        .route(
            "/api/admin/bindings",
            post(crate::provider::api_bind_provider),
        )
        .route(
            "/api/admin/api-keys",
            get(crate::api_key::api_list_api_keys).post(crate::api_key::api_create_api_key),
        )
        .route("/v1/responses", post(crate::gateway::openai_responses))
        .route("/v1/chat/completions", post(crate::gateway::openai_chat))
        .route("/v1/embeddings", post(crate::gateway::openai_embeddings))
        .route("/v1/messages", post(crate::gateway::anthropic_messages))
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

#[allow(unused_imports)]
pub use crate::storage::hash_password;
