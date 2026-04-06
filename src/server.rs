use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Router,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::config::GatewayState;
use crate::types::{AppConfig, AppState};

pub async fn run(config: AppConfig) -> Result<()> {
    let config_path = std::path::Path::new(&config.config_path);
    let gateway = GatewayState::load(config_path)
        .await
        .with_context(|| format!("load config: {}", config.config_path))?;
    let state = Arc::new(AppState::new(config.clone(), gateway.clone()));
    let (core_sync_tx, mut core_sync_rx) = mpsc::unbounded_channel();
    gateway.set_core_sync_notifier(core_sync_tx).await;
    state.sync_core_pools().await?;

    let sync_state = state.clone();
    tokio::spawn(async move {
        while core_sync_rx.recv().await.is_some() {
            while core_sync_rx.try_recv().is_ok() {}

            if let Err(error) = sync_state.sync_core_pools().await {
                warn!(error = %error, "failed to sync core pools after config change");
            }
        }
    });

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
        .route("/api/admin/modes", get(crate::service::api_list_modes))
        .route(
            "/api/admin/preferences/default-mode",
            post(crate::service::api_set_default_mode),
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
            get(crate::api_key::api_list_api_keys)
                .post(crate::api_key::api_create_api_key)
                .patch(crate::api_key::api_update_api_key_service),
        )
        .route("/v1/responses", post(crate::gateway::openai_responses))
        .route("/v1/chat/completions", post(crate::gateway::openai_chat))
        .route("/v1/embeddings", post(crate::gateway::openai_embeddings))
        .route("/v1/messages", post(crate::gateway::anthropic_messages));

    let app = app.with_state(state).layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
