use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde_json::json;

use crate::authz::is_admin_authorized;
use crate::dto::{ApiResponse, CreateServiceReq, ModeSummaryOut, ServiceOut, SetDefaultModeReq};
use crate::types::AppState;

#[derive(serde::Deserialize)]
pub(crate) struct ListModesQuery {
    pub(crate) detailed: Option<bool>,
}

pub(crate) async fn api_list_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let rows: Vec<ServiceOut> = state
        .gateway
        .list_services()
        .await
        .into_iter()
        .map(|(id, name)| ServiceOut { id, name })
        .collect();

    axum::Json(ApiResponse {
        success: true,
        data: rows,
    })
    .into_response()
}

pub(crate) async fn api_create_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<CreateServiceReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    state.gateway.create_service(&req.id, &req.name).await;
    let _ = state.gateway.persist_if_dirty().await;

    axum::Json(ApiResponse {
        success: true,
        data: json!({"id": req.id, "name": req.name}),
    })
    .into_response()
}

pub(crate) async fn api_list_modes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListModesQuery>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let modes = state.gateway.list_mode_views().await;
    let data = if query.detailed.unwrap_or(false) {
        json!(modes)
    } else {
        let rows: Vec<ModeSummaryOut> = modes
            .into_iter()
            .map(|mode| ModeSummaryOut {
                id: mode.id,
                name: mode.name,
                routing_strategy: mode.routing_strategy,
                is_default: mode.is_default,
                provider_count: mode.providers.len(),
                provider_names: mode
                    .providers
                    .into_iter()
                    .map(|provider| provider.name)
                    .collect(),
            })
            .collect();
        json!(rows)
    };

    axum::Json(ApiResponse {
        success: true,
        data,
    })
    .into_response()
}

pub(crate) async fn api_set_default_mode(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<SetDefaultModeReq>,
) -> impl IntoResponse {
    if !is_admin_authorized(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state.gateway.set_default_mode(&req.mode_id).await {
        Ok(_) => {
            let _ = state.gateway.persist_if_dirty().await;
            axum::Json(ApiResponse {
                success: true,
                data: json!({"mode_id": req.mode_id}),
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
    use super::{ListModesQuery, api_list_modes, api_set_default_mode};
    use crate::config::GatewayState;
    use crate::dto::SetDefaultModeReq;
    use crate::types::{AppConfig, AppState};
    use axum::Json;
    use axum::body::to_bytes;
    use axum::extract::{Query, State};
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
    async fn api_list_modes_unauthorized_without_token_header() {
        let state = test_state("secret").await;
        let response = api_list_modes(
            State(state),
            HeaderMap::new(),
            Query(ListModesQuery { detailed: None }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_list_modes_returns_detailed_data_when_requested() {
        let state = test_state("secret").await;
        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", HeaderValue::from_static("secret"));

        let response = api_list_modes(
            State(state),
            headers,
            Query(ListModesQuery {
                detailed: Some(true),
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
        let data = json
            .get("data")
            .and_then(Value::as_array)
            .expect("data array");
        assert_eq!(data.len(), 2);
        assert!(data[0].get("providers").is_some());
    }

    #[tokio::test]
    async fn api_set_default_mode_returns_bad_request_for_unknown_mode() {
        let state = test_state("secret").await;
        let mut headers = HeaderMap::new();
        headers.insert("x-admin-token", HeaderValue::from_static("secret"));

        let response = api_set_default_mode(
            State(state),
            headers,
            Json(SetDefaultModeReq {
                mode_id: "missing".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
