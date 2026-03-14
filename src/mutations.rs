use sqlx::SqlitePool;

pub(crate) async fn upsert_service(
    pool: &SqlitePool,
    id: &str,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR REPLACE INTO services(id, name) VALUES(?, ?)")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await?;

    Ok(())
}

pub(crate) async fn create_provider(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    endpoint_id: &str,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO providers(name, provider_type, endpoint_id, base_url, api_key, model_mapping, is_enabled)
         VALUES(?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(name)
    .bind(provider_type)
    .bind(endpoint_id)
    .bind(base_url.unwrap_or(""))
    .bind(api_key)
    .bind(model_mapping.unwrap_or(""))
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

pub(crate) async fn bind_provider_to_service(
    pool: &SqlitePool,
    service_id: &str,
    provider_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)",
    )
    .bind(service_id)
    .bind(provider_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn upsert_api_key_limits(
    pool: &SqlitePool,
    key: &str,
    service_id: &str,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR REPLACE INTO api_keys(key, service_id, quota_limit, used_quota, is_active)
         VALUES(?, ?, ?, COALESCE((SELECT used_quota FROM api_keys WHERE key = ?), 0), 1)",
    )
    .bind(key)
    .bind(service_id)
    .bind(quota_limit)
    .bind(key)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT OR REPLACE INTO api_key_limits(api_key, qps_limit, concurrency_limit)
         VALUES(?, ?, ?)",
    )
    .bind(key)
    .bind(qps_limit)
    .bind(concurrency_limit)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn create_provider_and_bind_default(
    pool: &SqlitePool,
    name: &str,
    provider_type: &str,
    endpoint_id: &str,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) {
    if name.trim().is_empty() || endpoint_id.trim().is_empty() {
        return;
    }

    let provider_id = sqlx::query(
        "INSERT INTO providers(name, provider_type, endpoint_id, base_url, api_key, model_mapping, is_enabled)
         VALUES(?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(name.trim())
    .bind(provider_type.trim())
    .bind(endpoint_id.trim())
    .bind(base_url.unwrap_or(""))
    .bind(api_key.trim())
    .bind(model_mapping.unwrap_or(""))
    .execute(pool)
    .await;

    if let Ok(result) = provider_id {
        let pid = result.last_insert_rowid();
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO services(id, name) VALUES('default', 'Default Service')",
        )
        .execute(pool)
        .await;

        let _ = sqlx::query(
            "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES('default', ?)",
        )
        .bind(pid)
        .execute(pool)
        .await;
    }
}

pub(crate) async fn delete_provider(pool: &SqlitePool, id: i64) {
    let _ = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await;
}

pub(crate) async fn delete_service(pool: &SqlitePool, service_id: &str, token_count: i64) {
    if token_count > 0 {
        let _ = sqlx::query(
            "DELETE FROM api_key_limits WHERE api_key IN (SELECT key FROM api_keys WHERE service_id = ?)",
        )
        .bind(service_id)
        .execute(pool)
        .await;

        let _ = sqlx::query("DELETE FROM api_keys WHERE service_id = ?")
            .bind(service_id)
            .execute(pool)
            .await;
    }

    let _ = sqlx::query("DELETE FROM service_providers WHERE service_id = ?")
        .bind(service_id)
        .execute(pool)
        .await;

    let _ = sqlx::query("DELETE FROM services WHERE id = ?")
        .bind(service_id)
        .execute(pool)
        .await;
}

pub(crate) async fn delete_api_key_and_maybe_service(
    pool: &SqlitePool,
    key: &str,
    service_id: Option<String>,
    delete_service: bool,
) {
    let _ = sqlx::query("DELETE FROM api_key_limits WHERE api_key = ?")
        .bind(key)
        .execute(pool)
        .await;

    let _ = sqlx::query("DELETE FROM api_keys WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await;

    if delete_service && let Some(service_id) = service_id && service_id != "default" {
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE service_id = ?")
            .bind(&service_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if remaining == 0 {
            let _ = sqlx::query("DELETE FROM service_providers WHERE service_id = ?")
                .bind(&service_id)
                .execute(pool)
                .await;
            let _ = sqlx::query("DELETE FROM services WHERE id = ?")
                .bind(&service_id)
                .execute(pool)
                .await;
        }
    }
}

pub(crate) async fn create_api_key_with_service(
    pool: &SqlitePool,
    name: &str,
    provider_ids: &[i64],
    key: &str,
    service_id: &str,
    service_name: &str,
) -> Result<(), sqlx::Error> {
    let _ = sqlx::query(
        "INSERT OR IGNORE INTO services(id, name) VALUES('default', 'Default Service')",
    )
    .execute(pool)
    .await;

    if !provider_ids.is_empty() {
        let _ = sqlx::query("INSERT OR IGNORE INTO services(id, name) VALUES(?, ?)")
            .bind(service_id)
            .bind(service_name)
            .execute(pool)
            .await;

        for provider_id in provider_ids {
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO service_providers(service_id, provider_id) VALUES(?, ?)",
            )
            .bind(service_id)
            .bind(provider_id)
            .execute(pool)
            .await;
        }
    }

    sqlx::query("INSERT INTO api_keys(name, key, service_id, is_active) VALUES(?, ?, ?, 1)")
        .bind(name.trim())
        .bind(key)
        .bind(service_id)
        .execute(pool)
        .await?;

    Ok(())
}
