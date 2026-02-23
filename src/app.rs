use std::{net::SocketAddr, sync::Arc, time::Instant};

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
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind: String,
    pub db_url: String,
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
            openai_base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
            openai_api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            openai_model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
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
    client: reqwest::Client,
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
        client: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/", get(home))
        .route("/login", get(login_page).post(login))
        .route("/logout", post(logout))
        .route("/admin", get(admin_page))
        .route("/admin/stats", get(admin_stats_partial))
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(openai_chat))
        .route("/v1/messages", post(anthropic_messages))
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

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

    let admin_exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'admin'")
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

fn hash_password(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

fn html_layout(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <script src="https://unpkg.com/htmx.org@1.9.12"></script>
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://cdn.jsdelivr.net/npm/daisyui@4.12.10/dist/full.min.css" rel="stylesheet" type="text/css" />
  <script>
    tailwind.config = {{
      theme: {{
        extend: {{
          colors: {{
            brand: '#3C6E71',
            brandLight: '#D9E6E7'
          }}
        }}
      }}
    }}
  </script>
</head>
<body class="bg-base-200 min-h-screen">{body}</body>
</html>"#
    )
}

async fn health() -> impl IntoResponse {
    Json(json!({"status":"ok","name":"UniGateway"}))
}

async fn home() -> impl IntoResponse {
    Redirect::to("/admin")
}

async fn login_page() -> impl IntoResponse {
    Html(html_layout(
        "UniGateway Login",
        r#"
<div class="min-h-screen flex items-center justify-center px-4">
  <div class="card w-full max-w-md bg-base-100 shadow-xl">
    <div class="card-body">
      <h1 class="card-title text-brand text-2xl">UniGateway</h1>
      <p class="text-sm text-base-content/70">默认管理员：admin / admin123</p>
      <form method="post" action="/login" class="space-y-3 mt-4">
        <input class="input input-bordered w-full" name="username" placeholder="用户名" value="admin" />
        <input class="input input-bordered w-full" type="password" name="password" placeholder="密码" />
        <button class="btn w-full bg-brand text-white border-none hover:opacity-90" type="submit">登录</button>
      </form>
    </div>
  </div>
</div>
"#,
    ))
}

fn get_cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|part| {
                let item = part.trim();
                item.strip_prefix("unigateway_session=").map(|v| v.to_string())
            })
        })
}

async fn login(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    let user = sqlx::query_as::<_, (i64, String)>("SELECT id, password_hash FROM users WHERE username = ?")
        .bind(&form.username)
        .fetch_optional(&state.pool)
        .await;

    let Ok(Some((user_id, password_hash))) = user else {
        return Html(html_layout("登录失败", "<div class='p-8'><p>用户名或密码错误</p><a class='link text-brand' href='/login'>返回登录</a></div>")).into_response();
    };

    if hash_password(&form.password) != password_hash {
        return Html(html_layout("登录失败", "<div class='p-8'><p>用户名或密码错误</p><a class='link text-brand' href='/login'>返回登录</a></div>")).into_response();
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
    if let Ok(cookie) =
        "unigateway_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".parse()
    {
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
    if !ensure_login(&state.pool, &headers).await {
        return Redirect::to("/login").into_response();
    }

    let page = html_layout(
        "UniGateway Admin",
        r#"
<div class="navbar bg-base-100 shadow-sm px-6">
  <div class="flex-1">
    <a class="text-xl font-bold text-brand">UniGateway</a>
  </div>
  <div class="flex-none">
    <form method="post" action="/logout"><button class="btn btn-sm">退出</button></form>
  </div>
</div>

<div class="p-6 space-y-6 max-w-5xl mx-auto">
  <div class="alert bg-brandLight text-brand border-none">
    <span>轻量开源版：OpenAI + Anthropic 网关，SQLite 统计。</span>
  </div>

  <div
    id="stats-box"
    hx-get="/admin/stats"
    hx-trigger="load, every 10s"
    class="grid grid-cols-1 md:grid-cols-3 gap-4"
  ></div>

  <div class="card bg-base-100 shadow">
    <div class="card-body">
      <h2 class="card-title">接口</h2>
      <ul class="list-disc list-inside text-sm space-y-1">
        <li>POST /v1/chat/completions (OpenAI 兼容)</li>
        <li>POST /v1/messages (Anthropic 兼容)</li>
        <li>GET /v1/models</li>
        <li>GET /health</li>
      </ul>
    </div>
  </div>
</div>
"#,
    );

    Html(page).into_response()
}

async fn admin_stats_partial(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !ensure_login(&state.pool, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let openai_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let anthropic_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    let content = format!(
        r#"
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">总请求</div>
  <div class="stat-value text-brand">{total}</div>
</div>
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">OpenAI 兼容</div>
  <div class="stat-value text-brand">{openai_count}</div>
</div>
<div class="stat bg-base-100 rounded-box shadow">
  <div class="stat-title">Anthropic 兼容</div>
  <div class="stat-value text-brand">{anthropic_count}</div>
</div>
"#
    );

    Html(content).into_response()
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
    Json(mut payload): Json<Value>,
) -> Response {
    let start = Instant::now();

    if payload.get("model").is_none() {
        payload["model"] = Value::String(state.config.openai_model.clone());
    }

    let url = format!(
        "{}/v1/chat/completions",
        state.config.openai_base_url.trim_end_matches('/')
    );

    let mut req = state.client.post(url).json(&payload);
    req = req.header("content-type", "application/json");

    let auth = headers
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

    if let Some(v) = auth {
        req = req.header(header::AUTHORIZATION, v);
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let bytes = resp.bytes().await.unwrap_or_default();
            record_stat(
                &state.pool,
                "openai",
                "/v1/chat/completions",
                status.as_u16() as i64,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (status, bytes).into_response()
        }
        Err(err) => {
            record_stat(
                &state.pool,
                "openai",
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
    Json(mut payload): Json<Value>,
) -> Response {
    let start = Instant::now();

    if payload.get("model").is_none() {
        payload["model"] = Value::String(state.config.anthropic_model.clone());
    }

    let url = format!("{}/v1/messages", state.config.anthropic_base_url.trim_end_matches('/'));
    let mut req = state
        .client
        .post(url)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&payload);

    if let Some(v) = headers
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
    {
        req = req.header("x-api-key", v);
    }

    match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let bytes = resp.bytes().await.unwrap_or_default();
            record_stat(
                &state.pool,
                "anthropic",
                "/v1/messages",
                status.as_u16() as i64,
                start.elapsed().as_millis() as i64,
            )
            .await;
            (status, bytes).into_response()
        }
        Err(err) => {
            record_stat(
                &state.pool,
                "anthropic",
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

async fn record_stat(pool: &SqlitePool, provider: &str, endpoint: &str, status_code: i64, latency_ms: i64) {
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
