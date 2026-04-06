use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiKeyOut, ApiResponse, CreateApiKeyReq, UpdateApiKeyServiceReq};
use crate::types::AppState;

pub(crate) async fn api_list_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ApiKeyOut> = state
        .gateway
        .list_api_keys()
        .await
        .into_iter()
        .map(|a| ApiKeyOut {
            key: a.key,
            service_id: a.service_id,
            quota_limit: a.quota_limit,
            used_quota: a.used_quota,
            is_active: if a.is_active { 1 } else { 0 },
            qps_limit: a.qps_limit,
            concurrency_limit: a.concurrency_limit,
        })
        .collect();

    axum::Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<CreateApiKeyReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    state
        .gateway
        .create_api_key(
            &req.key,
            &req.service_id,
            req.quota_limit,
            req.qps_limit,
            req.concurrency_limit,
        )
        .await;
    let _ = state.gateway.persist_if_dirty().await;

    axum::Json(ApiResponse {
        success: true,
        data: json!({"key": req.key, "service_id": req.service_id}),
    })
    .into_response()
}

pub(crate) async fn api_update_api_key_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<UpdateApiKeyServiceReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state
        .gateway
        .rebind_api_key_service(&req.key, &req.service_id)
        .await
    {
        Ok(_) => {
            let _ = state.gateway.persist_if_dirty().await;
            axum::Json(ApiResponse {
                success: true,
                data: json!({"key": req.key, "service_id": req.service_id}),
            })
            .into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"success": false, "error": err.to_string()})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::api_update_api_key_service;
    use crate::config::GatewayState;
    use crate::dto::UpdateApiKeyServiceReq;
    use crate::types::{AppConfig, AppState};
    use axum::Json;
    use axum::body::to_bytes;
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use serde_json::Value;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn test_state(admin_token: &str) -> Arc<AppState> {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let gateway = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        gateway.create_service("fast", "Fast").await;
        gateway.create_service("strong", "Strong").await;
        gateway
            .create_api_key("ugk_test_key", "fast", Some(10), Some(1.5), Some(2))
            .await;

        let config = AppConfig {
            bind: "127.0.0.1:3210".to_string(),
            config_path: config_path.to_string_lossy().to_string(),
            admin_token: admin_token.to_string(),
            openai_base_url: String::new(),
            openai_api_key: String::new(),
            openai_model: String::new(),
            anthropic_base_url: String::new(),
            anthropic_api_key: String::new(),
            anthropic_model: String::new(),
        };

        Arc::new(AppState::new(config, gateway))
    }

    #[tokio::test]
    async fn api_update_api_key_service_requires_admin_token() {
        let state = test_state("secret").await;
        let response = api_update_api_key_service(
            State(state),
            HeaderMap::new(),
            Json(UpdateApiKeyServiceReq {
                key: "ugk_test_key".to_string(),
                service_id: "strong".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_update_api_key_service_returns_bad_request_for_unknown_key() {
        let state = test_state("secret").await;
        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", HeaderValue::from_static("secret"));

        let response = api_update_api_key_service(
            State(state),
            headers,
            Json(UpdateApiKeyServiceReq {
                key: "ugk_missing".to_string(),
                service_id: "strong".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn api_update_api_key_service_rebinds_successfully() {
        let state = test_state("secret").await;
        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", HeaderValue::from_static("secret"));

        let response = api_update_api_key_service(
            State(state.clone()),
            headers,
            Json(UpdateApiKeyServiceReq {
                key: "ugk_test_key".to_string(),
                service_id: "strong".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let json: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json.get("success").and_then(Value::as_bool), Some(true));
        assert_eq!(
            json.pointer("/data/service_id").and_then(Value::as_str),
            Some("strong")
        );

        let keys = state.gateway.list_api_keys().await;
        let key = keys
            .iter()
            .find(|item| item.key == "ugk_test_key")
            .expect("key exists");
        assert_eq!(key.service_id, "strong");
        assert_eq!(key.quota_limit, Some(10));
        assert_eq!(key.qps_limit, Some(1.5));
        assert_eq!(key.concurrency_limit, Some(2));
    }
}
