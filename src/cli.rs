use anyhow::{Context, Result};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::path::Path;

use crate::app::hash_password;

pub async fn init_admin(db_url: &str, username: &str, password: &str) -> Result<()> {
    let pool = connect(db_url).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "INSERT INTO users(username, password_hash) VALUES(?, ?)
         ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash",
    )
    .bind(username)
    .bind(hash_password(password))
    .execute(&pool)
    .await?;

    Ok(())
}

pub async fn create_service(db_url: &str, service_id: &str, name: &str) -> Result<()> {
    let pool = connect(db_url).await?;
    ensure_management_schema(&pool).await?;

    sqlx::query("INSERT OR REPLACE INTO services(id, name) VALUES(?, ?)")
        .bind(service_id)
        .bind(name)
        .execute(&pool)
        .await?;

    Ok(())
}

pub async fn create_provider(
    db_url: &str,
    name: &str,
    provider_type: &str,
    endpoint_id: Option<&str>,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) -> Result<i64> {
    let pool = connect(db_url).await?;
    ensure_management_schema(&pool).await?;

    let result = sqlx::query(
        "INSERT INTO providers(name, provider_type, endpoint_id, base_url, api_key, model_mapping, is_enabled)
         VALUES(?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(name)
    .bind(provider_type)
    .bind(endpoint_id.unwrap_or(""))
    .bind(base_url.unwrap_or(""))
    .bind(api_key)
    .bind(model_mapping.unwrap_or(""))
    .execute(&pool)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn bind_provider(db_url: &str, service_id: &str, provider_id: i64) -> Result<()> {
    let pool = connect(db_url).await?;
    ensure_management_schema(&pool).await?;

    sqlx::query("INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)")
        .bind(service_id)
        .bind(provider_id)
        .execute(&pool)
        .await?;

    Ok(())
}

pub async fn create_api_key(
    db_url: &str,
    key: &str,
    service_id: &str,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
) -> Result<()> {
    let pool = connect(db_url).await?;
    ensure_management_schema(&pool).await?;

    sqlx::query(
        "INSERT OR REPLACE INTO api_keys(key, service_id, quota_limit, used_quota, is_active)
         VALUES(?, ?, ?, COALESCE((SELECT used_quota FROM api_keys WHERE key = ?), 0), 1)",
    )
    .bind(key)
    .bind(service_id)
    .bind(quota_limit)
    .bind(key)
    .execute(&pool)
    .await?;

    sqlx::query(
        "INSERT OR REPLACE INTO api_key_limits(api_key, qps_limit, concurrency_limit)
         VALUES(?, ?, ?)",
    )
    .bind(key)
    .bind(qps_limit)
    .bind(concurrency_limit)
    .execute(&pool)
    .await?;

    Ok(())
}

pub async fn print_metrics_snapshot(db_url: &str) -> Result<()> {
    let pool = connect(db_url).await?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
    let openai_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(0);
    let anthropic_total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(&pool)
            .await
            .unwrap_or(0);

    println!("unigateway_requests_total {}", total);
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}",
        openai_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}",
        anthropic_total
    );

    Ok(())
}

async fn connect(db_url: &str) -> Result<SqlitePool> {
    // 如果数据库文件不存在，自动创建
    if db_url.starts_with("sqlite://") {
        let db_path = db_url.strip_prefix("sqlite://").unwrap();
        if !Path::new(db_path).exists() {
            std::fs::File::create(db_path)
                .with_context(|| format!("failed to create database file: {}", db_path))?;
        }
    }

    SqlitePoolOptions::new()
        .max_connections(2)
        .connect(db_url)
        .await
        .with_context(|| format!("failed to connect sqlite: {}", db_url))
}

async fn ensure_management_schema(pool: &SqlitePool) -> Result<()> {
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

    Ok(())
}
