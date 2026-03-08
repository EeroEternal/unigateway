use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct ApiResponse<T: Serialize> {
    pub(crate) success: bool,
    pub(crate) data: T,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ProviderDetailRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: Option<String>,
    pub(crate) base_url: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ServiceSummaryRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ApiKeyListRow {
    pub(crate) key: String,
    pub(crate) name: Option<String>,
    pub(crate) service_id: String,
    pub(crate) service_name: Option<String>,
    pub(crate) created_at: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ApiKeyDetailRow {
    pub(crate) name: Option<String>,
    pub(crate) key: String,
    pub(crate) service_id: String,
    pub(crate) service_name: Option<String>,
    pub(crate) created_at: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ProviderListRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) service_count: i64,
    pub(crate) service_ids: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ProviderOptionRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) provider_type: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ServiceDetailRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ServiceDetailProviderRow {
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ServiceTokenRow {
    pub(crate) name: Option<String>,
    pub(crate) key: String,
    pub(crate) created_at: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ServiceListRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) provider_count: i64,
    pub(crate) provider_names: Option<String>,
    pub(crate) token_count: i64,
    pub(crate) token_names: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct LogRow {
    pub(crate) created_at: String,
    pub(crate) endpoint: String,
    pub(crate) provider: String,
    pub(crate) status_code: i64,
    pub(crate) latency_ms: i64,
}

pub(crate) struct DashboardStats {
    pub(crate) total: i64,
    pub(crate) api_keys: i64,
    pub(crate) providers: i64,
    pub(crate) services: i64,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ServiceOut {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ProviderOut {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: Option<String>,
    pub(crate) base_url: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub(crate) struct ApiKeyOut {
    pub(crate) key: String,
    pub(crate) service_id: String,
    pub(crate) quota_limit: Option<i64>,
    pub(crate) used_quota: i64,
    pub(crate) is_active: i64,
    pub(crate) qps_limit: Option<f64>,
    pub(crate) concurrency_limit: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct CreateServiceReq {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct CreateProviderReq {
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: String,
    pub(crate) base_url: Option<String>,
    pub(crate) api_key: String,
    pub(crate) model_mapping: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BindProviderReq {
    pub(crate) service_id: String,
    pub(crate) provider_id: i64,
}

#[derive(Deserialize)]
pub(crate) struct CreateApiKeyReq {
    pub(crate) key: String,
    pub(crate) service_id: String,
    pub(crate) quota_limit: Option<i64>,
    pub(crate) qps_limit: Option<f64>,
    pub(crate) concurrency_limit: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct DeleteApiKeyQuery {
    pub(crate) delete_service: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct DeleteServiceQuery {
    pub(crate) force: Option<i64>,
}
