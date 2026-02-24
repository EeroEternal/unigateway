use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::{Duration, Instant}};

use anyhow::{Context, Result};
use axum::{
    extract::{Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::protocol::{
    anthropic_payload_to_chat_request, chat_response_to_anthropic_json,
    chat_response_to_openai_json, invoke_with_connector, openai_payload_to_chat_request,
    UpstreamProtocol,
};
use crate::ui;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: String,
    pub db_url: String,
    pub enable_ui: bool,
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
struct AppState {
    pool: SqlitePool,
    config: AppConfig,
    api_key_runtime: Arc<Mutex<HashMap<String, RuntimeRateState>>>,
    service_rr: Arc<Mutex<HashMap<String, usize>>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GatewayApiKey {
    key: String,
    service_id: String,
    quota_limit: Option<i64>,
    used_quota: i64,
    is_active: i64,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ServiceProvider {
    name: String,
    base_url: Option<String>,
    api_key: Option<String>,
    model_mapping: Option<String>,
}

#[derive(Debug)]
struct RuntimeRateState {
    window_started_at: Instant,
    request_count: u64,
    in_flight: u64,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct ModelList {
    object: &'static str,
    data: Vec<ModelItem>,
}

#[derive(Serialize)]
struct ModelItem {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: &'static str,
}

pub async fn run(config: AppConfig) -> Result<()> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.db_url)
        .await
        .with_context(|| format!("failed to connect sqlite: {}", config.db_url))?;

    init_db(&pool).await?;

    let state = AppState {
        pool,
        config: config.clone(),
        api_key_runtime: Arc::new(Mutex::new(HashMap::new())),
        service_rr: Arc::new(Mutex::new(HashMap::new())),
    };

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/messages", post(anthropic_messages));

    if config.enable_ui {
        app = app
            .route("/", get(home))
            .route("/login", get(login_page).post(login))
            .route("/logout", post(logout))
            .route("/admin", get(admin_page))
            .route("/admin/stats", get(admin_stats_partial));
    }

    let app = app.with_state(Arc::new(state)).layer(TraceLayer::new_for_http());

    let addr: SocketAddr = config.bind.parse().context("invalid UNIGATEWAY_BIND")?;
    let listener = TcpListener::bind(addr).await?;
    info!("UniGateway listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn init_db(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS request_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            endpoint TEXT NOT NULL,
            status_code INTEGER NOT NULL,
            latency_ms INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    // --- New Tables for v0.2 Governance ---

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS services (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            routing_strategy TEXT NOT NULL DEFAULT 'round_robin',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS providers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url TEXT,
            api_key TEXT,
            model_mapping TEXT,
            weight INTEGER DEFAULT 1,
            is_enabled BOOLEAN DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS service_providers (
            service_id TEXT NOT NULL,
            provider_id INTEGER NOT NULL,
            PRIMARY KEY (service_id, provider_id),
            FOREIGN KEY(service_id) REFERENCES services(id),
            FOREIGN KEY(provider_id) REFERENCES providers(id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_keys (
            key TEXT PRIMARY KEY,
            service_id TEXT NOT NULL,
            name TEXT,
            quota_limit INTEGER,
            used_quota INTEGER DEFAULT 0,
            is_active BOOLEAN DEFAULT 1,
            expired_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(service_id) REFERENCES services(id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_key_limits (
            api_key TEXT PRIMARY KEY,
            qps_limit REAL,
            concurrency_limit INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(api_key) REFERENCES api_keys(key)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id TEXT NOT NULL,
            service_id TEXT,
            provider_id INTEGER,
            model TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            total_tokens INTEGER,
            latency_ms INTEGER,
            status_code INTEGER,
            client_ip TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(pool)
    .await?;

    // --- End New Tables ---

    let admin_exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'admin'")
            .fetch_one(pool)
            .await?;

    if admin_exists == 0 {
        let hash = hash_password("admin123");
        sqlx::query("INSERT INTO users(username, password_hash) VALUES(?, ?)")
            .bind("admin")
            .bind(hash)
            .execute(pool)
            .await?;
    }

    Ok(())
}

pub fn hash_password(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","name":"UniGateway"}))
}

async fn home() -> impl IntoResponse {
    Redirect::to("/admin")
}

async fn login_page() -> impl IntoResponse {
        Html(ui::login_page())
}

fn get_cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|part| {
                let item = part.trim();
                item.strip_prefix("unigateway_session=")
                    .map(|v| v.to_string())
            })
        })
}

async fn login(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let user = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, password_hash FROM users WHERE username = ?",
    )
    .bind(&form.username)
    .fetch_optional(&state.pool)
    .await;

    let Ok(Some((user_id, password_hash))) = user else {
        return Html(ui::login_error_page()).into_response();
    };

    if hash_password(&form.password) != password_hash {
        return Html(ui::login_error_page()).into_response();
    }

    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect();

    if sqlx::query("INSERT INTO sessions(token, user_id) VALUES(?, ?)")
        .bind(&token)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, "session create failed").into_response();
    }

    let mut headers = HeaderMap::new();
    if let Ok(cookie) =
        format!("unigateway_session={token}; Path=/; HttpOnly; SameSite=Lax").parse()
    {
        headers.insert(header::SET_COOKIE, cookie);
    }

    (headers, Redirect::to("/admin")).into_response()
}

async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = get_cookie_token(&headers) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.pool)
            .await;
    }

    let mut response = Redirect::to("/login").into_response();
    if let Ok(cookie) = "unigateway_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".parse() {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

async fn ensure_login(pool: &SqlitePool, headers: &HeaderMap) -> bool {
    let Some(token) = get_cookie_token(headers) else {
        return false;
    };

    match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE token = ?")
        .bind(token)
        .fetch_one(pool)
        .await
    {
        Ok(count) => count > 0,
        Err(_) => false,
    }
}

async fn admin_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
        if !state.config.enable_ui {
                return StatusCode::NOT_FOUND.into_response();
        }

    if !ensure_login(&state.pool, &headers).await {
        return Redirect::to("/login").into_response();
    }

        Html(ui::admin_page()).into_response()
}

async fn admin_stats_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.config.enable_ui {
        return StatusCode::NOT_FOUND.into_response();
    }

    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let openai_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let anthropic_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let content = ui::stats_partial(total, openai_count, anthropic_count);

    Html(content).into_response()
}

async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let openai_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);

    let anthropic_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let body = format!(
        "# TYPE unigateway_requests_total counter\nunigateway_requests_total {}\n# TYPE unigateway_requests_by_endpoint_total counter\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}\nunigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}\n",
        total, openai_total, anthropic_total
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

async fn models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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

async fn openai_chat(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let start = Instant::now();

    let api_key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            if state.config.openai_api_key.is_empty() {
                None
            } else {
                Some(format!("Bearer {}", state.config.openai_api_key))
            }
        });

    let token = api_key
        .as_deref()
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    let mut request = match openai_payload_to_chat_request(&payload, &state.config.openai_model) {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":format!("invalid request: {err}")}})),
            )
                .into_response();
        }
    };

    let mut upstream_base_url = state.config.openai_base_url.clone();
    let mut upstream_api_key = token.to_string();
    let mut provider_label = "openai".to_string();
    let mut release_gateway_key: Option<String> = None;

    if !token.is_empty() {
        match find_gateway_api_key(&state.pool, token).await {
            Ok(Some(gateway_key)) => {
                if gateway_key.is_active == 0 {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error":{"message":"api key is inactive"}})),
                    )
                        .into_response();
                }

                if let Some(quota_limit) = gateway_key.quota_limit {
                    if gateway_key.used_quota >= quota_limit {
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({"error":{"message":"api key quota exceeded"}})),
                        )
                            .into_response();
                    }
                }

                if let Err(resp) = acquire_runtime_limit(&state, &gateway_key).await {
                    return resp;
                }

                let provider = match select_provider_for_service(&state, &gateway_key.service_id, "openai").await {
                    Ok(Some(provider)) => provider,
                    Ok(None) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":{"message":"no provider bound for service/openai"}})),
                        )
                            .into_response();
                    }
                    Err(err) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error":{"message":format!("db error: {err}")}})),
                        )
                            .into_response();
                    }
                };

                let Some(base_url) = provider.base_url.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider base_url missing"}})),
                    )
                        .into_response();
                };
                let Some(provider_api_key) = provider.api_key.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider api_key missing"}})),
                    )
                        .into_response();
                };

                if let Some(mapped_model) = map_model_name(provider.model_mapping.as_deref(), &request.model) {
                    request.model = mapped_model;
                }

                upstream_base_url = base_url;
                upstream_api_key = provider_api_key;
                provider_label = provider.name;
                release_gateway_key = Some(gateway_key.key.clone());

                let _ = sqlx::query("UPDATE api_keys SET used_quota = COALESCE(used_quota, 0) + 1 WHERE key = ?")
                    .bind(&gateway_key.key)
                    .execute(&state.pool)
                    .await;
            }
            Ok(None) => {
            }
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":{"message":format!("db error: {err}")}})),
                )
                    .into_response();
            }
        }
    }

    if upstream_api_key.is_empty() {
        upstream_api_key = state.config.openai_api_key.clone();
    }
    if upstream_api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"missing upstream api key"}})),
        )
            .into_response();
    }

    match invoke_with_connector(
        UpstreamProtocol::OpenAi,
        &upstream_base_url,
        &upstream_api_key,
        &request,
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_openai_json(&resp);
            let status = StatusCode::OK;
            record_stat(&state.pool, &provider_label, "/v1/chat/completions", 200, start.elapsed().as_millis() as i64)
                .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/chat/completions",
                500,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err}")}})),
            )
                .into_response()
        }
    }
}

async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let start = Instant::now();

    let api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            if state.config.anthropic_api_key.is_empty() {
                None
            } else {
                Some(state.config.anthropic_api_key.clone())
            }
        })
        .unwrap_or_default();

    let mut request = match anthropic_payload_to_chat_request(&payload, &state.config.anthropic_model)
    {
        Ok(req) => req,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":format!("invalid request: {err}")}})),
            )
                .into_response();
        }
    };

    let mut upstream_base_url = state.config.anthropic_base_url.clone();
    let mut upstream_api_key = api_key.clone();
    let mut provider_label = "anthropic".to_string();
    let mut release_gateway_key: Option<String> = None;

    if !api_key.is_empty() {
        match find_gateway_api_key(&state.pool, &api_key).await {
            Ok(Some(gateway_key)) => {
                if gateway_key.is_active == 0 {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error":{"message":"api key is inactive"}})),
                    )
                        .into_response();
                }

                if let Some(quota_limit) = gateway_key.quota_limit {
                    if gateway_key.used_quota >= quota_limit {
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            Json(json!({"error":{"message":"api key quota exceeded"}})),
                        )
                            .into_response();
                    }
                }

                if let Err(resp) = acquire_runtime_limit(&state, &gateway_key).await {
                    return resp;
                }

                let provider = match select_provider_for_service(&state, &gateway_key.service_id, "anthropic").await {
                    Ok(Some(provider)) => provider,
                    Ok(None) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(json!({"error":{"message":"no provider bound for service/anthropic"}})),
                        )
                            .into_response();
                    }
                    Err(err) => {
                        release_runtime_inflight(&state, &gateway_key.key).await;
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({"error":{"message":format!("db error: {err}")}})),
                        )
                            .into_response();
                    }
                };

                let Some(base_url) = provider.base_url.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider base_url missing"}})),
                    )
                        .into_response();
                };
                let Some(provider_api_key) = provider.api_key.clone() else {
                    release_runtime_inflight(&state, &gateway_key.key).await;
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"provider api_key missing"}})),
                    )
                        .into_response();
                };

                if let Some(mapped_model) = map_model_name(provider.model_mapping.as_deref(), &request.model) {
                    request.model = mapped_model;
                }

                upstream_base_url = base_url;
                upstream_api_key = provider_api_key;
                provider_label = provider.name;
                release_gateway_key = Some(gateway_key.key.clone());

                let _ = sqlx::query("UPDATE api_keys SET used_quota = COALESCE(used_quota, 0) + 1 WHERE key = ?")
                    .bind(&gateway_key.key)
                    .execute(&state.pool)
                    .await;
            }
            Ok(None) => {
            }
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":{"message":format!("db error: {err}")}})),
                )
                    .into_response();
            }
        }
    }

    if upstream_api_key.is_empty() {
        upstream_api_key = state.config.anthropic_api_key.clone();
    }
    if upstream_api_key.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":"missing upstream api key"}})),
        )
            .into_response();
    }

    match invoke_with_connector(
        UpstreamProtocol::Anthropic,
        &upstream_base_url,
        &upstream_api_key,
        &request,
    )
    .await
    {
        Ok(resp) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            let body = chat_response_to_anthropic_json(&resp);
            let status = StatusCode::OK;
            record_stat(&state.pool, &provider_label, "/v1/messages", 200, start.elapsed().as_millis() as i64)
                .await;
            (status, Json(body)).into_response()
        }
        Err(err) => {
            if let Some(gateway_key) = release_gateway_key {
                release_runtime_inflight(&state, &gateway_key).await;
            }
            record_stat(
                &state.pool,
                &provider_label,
                "/v1/messages",
                500,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":format!("upstream error: {err}")}})),
            )
                .into_response()
        }
    }
}

async fn record_stat(
    pool: &SqlitePool,
    provider: &str,
    endpoint: &str,
    status_code: i64,
    latency_ms: i64,
) {
    let _ = sqlx::query(
        "INSERT INTO request_stats(provider, endpoint, status_code, latency_ms) VALUES(?, ?, ?, ?)",
    )
    .bind(provider)
    .bind(endpoint)
    .bind(status_code)
    .bind(latency_ms)
    .execute(pool)
    .await;
}

async fn find_gateway_api_key(pool: &SqlitePool, raw_key: &str) -> Result<Option<GatewayApiKey>> {
    let key = sqlx::query_as::<_, GatewayApiKey>(
        "SELECT
            k.key,
            k.service_id,
            k.quota_limit,
            COALESCE(k.used_quota, 0) AS used_quota,
            COALESCE(k.is_active, 1) AS is_active,
            l.qps_limit,
            l.concurrency_limit
         FROM api_keys k
         LEFT JOIN api_key_limits l ON l.api_key = k.key
         WHERE k.key = ?",
    )
    .bind(raw_key)
    .fetch_optional(pool)
    .await?;

    Ok(key)
}

async fn select_provider_for_service(
    state: &Arc<AppState>,
    service_id: &str,
    protocol: &str,
) -> Result<Option<ServiceProvider>> {
    let providers = sqlx::query_as::<_, ServiceProvider>(
        "SELECT p.name, p.base_url, p.api_key, p.model_mapping
         FROM providers p
         INNER JOIN service_providers sp ON sp.provider_id = p.id
         WHERE sp.service_id = ? AND COALESCE(p.is_enabled, 1) = 1 AND p.provider_type = ?
         ORDER BY p.id",
    )
    .bind(service_id)
    .bind(protocol)
    .fetch_all(&state.pool)
    .await?;

    if providers.is_empty() {
        return Ok(None);
    }

    let bucket = format!("{}:{}", service_id, protocol);
    let mut rr = state.service_rr.lock().await;
    let current_idx = rr.entry(bucket).or_insert(0usize);
    let provider = providers[*current_idx % providers.len()].clone();
    *current_idx = (*current_idx + 1) % providers.len();
    Ok(Some(provider))
}

async fn acquire_runtime_limit(state: &Arc<AppState>, gateway_key: &GatewayApiKey) -> std::result::Result<(), Response> {
    let mut runtime = state.api_key_runtime.lock().await;
    let entry = runtime
        .entry(gateway_key.key.clone())
        .or_insert_with(|| RuntimeRateState {
            window_started_at: Instant::now(),
            request_count: 0,
            in_flight: 0,
        });

    if entry.window_started_at.elapsed() >= Duration::from_secs(1) {
        entry.window_started_at = Instant::now();
        entry.request_count = 0;
    }

    if let Some(qps_limit) = gateway_key.qps_limit {
        if qps_limit > 0.0 && (entry.request_count as f64) >= qps_limit {
            return Err(
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error":{"message":"api key qps limit exceeded"}})),
                )
                    .into_response(),
            );
        }
    }

    if let Some(concurrency_limit) = gateway_key.concurrency_limit {
        if concurrency_limit > 0 && (entry.in_flight as i64) >= concurrency_limit {
            return Err(
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error":{"message":"api key concurrency limit exceeded"}})),
                )
                    .into_response(),
            );
        }
    }

    entry.request_count += 1;
    entry.in_flight += 1;

    Ok(())
}

async fn release_runtime_inflight(state: &Arc<AppState>, key: &str) {
    let mut runtime = state.api_key_runtime.lock().await;
    if let Some(entry) = runtime.get_mut(key) {
        if entry.in_flight > 0 {
            entry.in_flight -= 1;
        }
    }
}

fn map_model_name(model_mapping: Option<&str>, requested_model: &str) -> Option<String> {
    let Some(raw_mapping) = model_mapping else {
        return None;
    };

    if let Ok(value) = serde_json::from_str::<Value>(raw_mapping) {
        if let Some(mapped) = value.get(requested_model).and_then(Value::as_str) {
            return Some(mapped.to_string());
        }
        if let Some(default) = value.get("default").and_then(Value::as_str) {
            return Some(default.to_string());
        }
    }

    if !raw_mapping.trim().is_empty() && !raw_mapping.trim().starts_with('{') {
        return Some(raw_mapping.trim().to_string());
    }

    None
}
