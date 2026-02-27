use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::types::{AppState, GatewayApiKey, ServiceProvider};

pub(crate) async fn init_db(pool: &SqlitePool) -> Result<()> {
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
            endpoint_id TEXT,
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

    // 旧库轻量迁移：补齐 endpoint_id 列
    let _ = sqlx::query("ALTER TABLE providers ADD COLUMN endpoint_id TEXT")
        .execute(pool)
        .await;

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

pub(crate) async fn record_stat(
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

pub(crate) async fn find_gateway_api_key(
    pool: &SqlitePool,
    raw_key: &str,
) -> Result<Option<GatewayApiKey>> {
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

pub(crate) async fn select_provider_for_service(
    state: &Arc<AppState>,
    service_id: &str,
    protocol: &str,
) -> Result<Option<ServiceProvider>> {
    let providers = sqlx::query_as::<_, ServiceProvider>(
        "SELECT p.name, p.provider_type, p.endpoint_id, p.base_url, p.api_key, p.model_mapping
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

pub(crate) fn map_model_name(model_mapping: Option<&str>, requested_model: &str) -> Option<String> {
    let raw_mapping = model_mapping?;

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
