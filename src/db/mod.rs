use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub mod models;

pub async fn init_pool(db_url: &str) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await
        .with_context(|| format!("failed to connect sqlite: {}", db_url))?;

    init_schema(&pool).await?;
    Ok(pool)
}

async fn init_schema(pool: &SqlitePool) -> Result<()> {
    // 1. Users table
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

    // 2. Sessions table
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

    // 3. Request Stats table (Legacy)
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

    // 4. Services table
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

    // 5. Providers table
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

    // 6. Service Providers mapping table
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

    // 7. API Keys table
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

    // 8. Request Logs table (Detailed)
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

    // Init admin user
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
