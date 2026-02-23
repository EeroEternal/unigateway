use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    extract::{Json, State},
    routing::{delete, get, post},
    Router,
};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::AppConfig;
use crate::db::init_pool;
use crate::engine::Engine;
use crate::handlers::{admin, auth, chat, health, home};
use crate::db::models::{ModelList, ModelItem};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: AppConfig,
    pub engine: Engine,
}

pub async fn run(config: AppConfig) -> Result<()> {
    let pool = init_pool(&config.db_url).await?;
    let engine = Engine::new()?;

    let state = AppState {
        pool,
        config: config.clone(),
        engine,
    };

    let app = Router::new()
        // Public & Auth
        .route("/", get(home))
        .route("/login", get(auth::login_page).post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/health", get(health::health))
        
        // Admin
        .route("/admin", get(admin::admin_page))
        .route("/admin/stats", get(admin::admin_stats_partial))
        .route("/admin/providers", get(admin::providers_page).post(admin::create_provider))
        .route("/admin/providers/list", get(admin::providers_list))
        .route("/admin/providers/:id", delete(admin::delete_provider))
        
        // API (Legacy / v1)
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat::openai_chat))
        .route("/v1/messages", post(chat::anthropic_messages))
        
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn models(State(state): State<Arc<AppState>>) -> Json<ModelList> {
    Json(ModelList {
        object: "list",
        data: vec![
            ModelItem {
                id: state.config.openai_model.clone(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "openai",
            },
            ModelItem {
                id: state.config.anthropic_model.clone(),
                object: "model",
                created: chrono::Utc::now().timestamp(),
                owned_by: "anthropic",
            },
        ],
    })
}
