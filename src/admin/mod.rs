use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use unigateway_core::{UniGatewayEngine, engine::AimdSnapshot};

use crate::config::GatewayState;
use crate::types::AppState;

mod api_key;
mod mcp;
mod metrics;
mod provider;
mod service;

pub(crate) use self::mcp::run as run_mcp;

#[derive(Clone)]
pub(crate) struct AdminState {
    admin_token: String,
    gateway: Arc<GatewayState>,
    core_engine: Arc<UniGatewayEngine>,
}

impl AdminState {
    pub(crate) fn from_app_state(state: &AppState) -> Self {
        Self {
            admin_token: state.admin_token().to_string(),
            gateway: state.gateway.clone(),
            core_engine: state.core_engine.clone(),
        }
    }

    pub(crate) fn admin_token(&self) -> &str {
        self.admin_token.as_str()
    }

    pub(crate) fn gateway(&self) -> &GatewayState {
        self.gateway.as_ref()
    }

    pub(crate) async fn aimd_metrics(&self) -> std::collections::HashMap<String, AimdSnapshot> {
        self.core_engine.aimd_metrics().await
    }
}

pub(crate) fn router() -> Router<Arc<AdminState>> {
    Router::new()
        .route(
            "/api/admin/services",
            get(service::api_list_services).post(service::api_create_service),
        )
        .route("/api/admin/modes", get(service::api_list_modes))
        .route(
            "/api/admin/preferences/default-mode",
            post(service::api_set_default_mode),
        )
        .route(
            "/api/admin/providers",
            get(provider::api_list_providers).post(provider::api_create_provider),
        )
        .route("/v1/admin/queue_metrics", get(metrics::queue_metrics))
        .route("/api/admin/bindings", post(provider::api_bind_provider))
        .route(
            "/api/admin/api-keys",
            get(api_key::api_list_api_keys)
                .post(api_key::api_create_api_key)
                .patch(api_key::api_update_api_key_service),
        )
}
