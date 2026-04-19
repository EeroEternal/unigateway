use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use crate::config::GatewayState;
use crate::types::{AppConfig, AppState, GatewayRequestState, SystemState};

pub async fn run(config: AppConfig) -> Result<()> {
    let config_path = std::path::Path::new(&config.config_path);
    let gateway = GatewayState::load(config_path)
        .await
        .with_context(|| format!("load config: {}", config.config_path))?;
    let state = Arc::new(AppState::new(config.clone(), gateway.clone()));
    let admin_state = Arc::new(crate::admin::AdminState::from_app_state(state.as_ref()));
    let gateway_request_state = Arc::new(GatewayRequestState::from_app_state(state.as_ref()));
    let system_state = Arc::new(SystemState::from_app_state(state.as_ref()));
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
        .merge(crate::system::router().with_state(system_state))
        .merge(crate::gateway::router().with_state(gateway_request_state))
        .merge(crate::admin::router().with_state(admin_state));

    let app = app.layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(gateway))
        .await?;

    Ok(())
}

async fn shutdown_signal(gateway: Arc<GatewayState>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
    // Ensure any unflushed quotas/metadata are written before exit
    let _ = gateway.persist_if_dirty().await;
}
