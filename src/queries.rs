use sqlx::SqlitePool;

use crate::dto::{
    ApiKeyDetailRow, ApiKeyListRow, ApiKeyOut, DashboardStats, LogRow, ProviderDetailRow,
    ProviderListRow, ProviderOptionRow, ProviderOut, ServiceDetailProviderRow, ServiceDetailRow,
    ServiceListRow, ServiceOut, ServiceSummaryRow, ServiceTokenRow,
};

pub(crate) struct MetricsSnapshot {
    pub(crate) total: i64,
    pub(crate) openai_total: i64,
    pub(crate) anthropic_total: i64,
}

pub(crate) async fn find_provider_detail(
    pool: &SqlitePool,
    id: i64,
) -> Option<ProviderDetailRow> {
    sqlx::query_as(
        "SELECT id, name, provider_type, endpoint_id, base_url FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

pub(crate) async fn fetch_metrics_snapshot(pool: &SqlitePool) -> MetricsSnapshot {
    let total = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let openai_total = sqlx::query_scalar(
        "SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/chat/completions'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let anthropic_total =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_stats WHERE endpoint = '/v1/messages'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    MetricsSnapshot {
        total,
        openai_total,
        anthropic_total,
    }
}

pub(crate) async fn list_service_out(pool: &SqlitePool) -> Vec<ServiceOut> {
    sqlx::query_as("SELECT id, name FROM services ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

pub(crate) async fn list_provider_out(pool: &SqlitePool) -> Vec<ProviderOut> {
    sqlx::query_as(
        "SELECT id, name, provider_type, endpoint_id, base_url FROM providers ORDER BY id DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_api_key_out(pool: &SqlitePool) -> Vec<ApiKeyOut> {
    sqlx::query_as(
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
         ORDER BY k.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_services_by_provider(
    pool: &SqlitePool,
    provider_id: i64,
) -> Vec<ServiceSummaryRow> {
    sqlx::query_as(
        "SELECT s.id, s.name, s.created_at
         FROM service_providers sp
         JOIN services s ON s.id = sp.service_id
         WHERE sp.provider_id = ?
         ORDER BY s.created_at DESC",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn find_service_detail(
    pool: &SqlitePool,
    service_id: &str,
) -> Option<ServiceDetailRow> {
    sqlx::query_as("SELECT id, name, created_at FROM services WHERE id = ?")
        .bind(service_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None)
}

pub(crate) async fn list_providers_for_service(
    pool: &SqlitePool,
    service_id: &str,
) -> Vec<ServiceDetailProviderRow> {
    sqlx::query_as(
        "SELECT p.name, p.provider_type, p.endpoint_id
         FROM service_providers sp
         JOIN providers p ON p.id = sp.provider_id
         WHERE sp.service_id = ?
         ORDER BY p.name ASC",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_api_keys_for_service(
    pool: &SqlitePool,
    service_id: &str,
) -> Vec<ServiceTokenRow> {
    sqlx::query_as(
        "SELECT name, key, created_at FROM api_keys WHERE service_id = ? ORDER BY created_at DESC",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn find_api_key_detail(
    pool: &SqlitePool,
    api_key: &str,
) -> Option<ApiKeyDetailRow> {
    sqlx::query_as(
        "SELECT k.name, k.key, k.service_id, s.name, k.created_at
         FROM api_keys k
         LEFT JOIN services s ON s.id = k.service_id
         WHERE k.key = ?",
    )
    .bind(api_key)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

pub(crate) async fn list_provider_options(pool: &SqlitePool) -> Vec<ProviderOptionRow> {
    sqlx::query_as("SELECT id, name, provider_type FROM providers ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

pub(crate) async fn fetch_dashboard_stats(pool: &SqlitePool) -> DashboardStats {
    let total = sqlx::query_scalar("SELECT COUNT(*) FROM request_stats")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let api_keys = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let providers = sqlx::query_scalar("SELECT COUNT(*) FROM providers")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let services = sqlx::query_scalar("SELECT COUNT(*) FROM services")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    DashboardStats {
        total,
        api_keys,
        providers,
        services,
    }
}

pub(crate) async fn list_provider_rows(pool: &SqlitePool) -> Vec<ProviderListRow> {
    sqlx::query_as(
        "SELECT
                p.id,
                p.name,
                p.provider_type,
                p.endpoint_id,
                p.base_url,
                COUNT(sp.service_id) AS service_count,
                GROUP_CONCAT(sp.service_id, ', ') AS service_ids
             FROM providers p
             LEFT JOIN service_providers sp ON sp.provider_id = p.id
             GROUP BY p.id, p.name, p.provider_type, p.endpoint_id, p.base_url
             ORDER BY p.id DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_api_key_rows(pool: &SqlitePool) -> Vec<ApiKeyListRow> {
    sqlx::query_as(
        "SELECT k.key, k.name, k.service_id, s.name, k.created_at
             FROM api_keys k
             LEFT JOIN services s ON s.id = k.service_id
             ORDER BY k.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_service_rows(pool: &SqlitePool) -> Vec<ServiceListRow> {
    sqlx::query_as(
        "SELECT
            s.id,
            s.name,
            s.created_at,
            COUNT(DISTINCT sp.provider_id) AS provider_count,
            GROUP_CONCAT(DISTINCT p.name) AS provider_names,
            COUNT(DISTINCT k.key) AS token_count,
            GROUP_CONCAT(DISTINCT COALESCE(k.name, k.key)) AS token_names
         FROM services s
         LEFT JOIN service_providers sp ON sp.service_id = s.id
         LEFT JOIN providers p ON p.id = sp.provider_id
         LEFT JOIN api_keys k ON k.service_id = s.id
         GROUP BY s.id, s.name, s.created_at
         ORDER BY s.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn list_log_rows(pool: &SqlitePool) -> Vec<LogRow> {
    sqlx::query_as(
        "SELECT created_at, endpoint, provider, status_code, latency_ms
         FROM request_stats ORDER BY id DESC LIMIT 20",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub(crate) async fn count_api_keys_by_service(pool: &SqlitePool, service_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE service_id = ?")
        .bind(service_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
}

pub(crate) async fn find_service_id_for_api_key(
    pool: &SqlitePool,
    key: &str,
) -> Option<String> {
    sqlx::query_scalar("SELECT service_id FROM api_keys WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}
