use std::sync::Arc;
use std::time::Instant;

use axum::response::Response;
use unigateway_runtime::flow::RuntimeResponseResult;

use crate::middleware::{GatewayAuth, record_stat};
use crate::types::AppState;

use super::request_flow::PreparedGatewayRequest;

pub(super) async fn respond_prepared_runtime_result(
    state: &Arc<AppState>,
    prepared: &PreparedGatewayRequest<'_>,
    endpoint: &str,
    result: RuntimeResponseResult,
) -> Response {
    match prepared.auth.as_ref() {
        Some(auth) => {
            respond_authenticated_runtime_result(state, auth, endpoint, &prepared.start, result)
                .await
        }
        None => respond_env_runtime_result(state, endpoint, &prepared.start, result).await,
    }
}

pub(super) async fn respond_authenticated_runtime_result(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    result: RuntimeResponseResult,
) -> Response {
    match result {
        Ok(response) => gateway_success_response(state, auth, endpoint, start, response).await,
        Err(response) => gateway_error_response(state, auth, endpoint, start, response).await,
    }
}

pub(super) async fn respond_env_runtime_result(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    result: RuntimeResponseResult,
) -> Response {
    match result {
        Ok(response) => success_response(state, endpoint, start, response).await,
        Err(response) => error_response(state, endpoint, start, response).await,
    }
}

async fn gateway_success_response(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    auth.finalize(state).await;
    success_response(state, endpoint, start, response).await
}

async fn gateway_error_response(
    state: &Arc<AppState>,
    auth: &GatewayAuth,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    auth.release(state).await;
    error_response(state, endpoint, start, response).await
}

async fn success_response(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    record_stat(state, endpoint, 200, start).await;
    response
}

async fn error_response(
    state: &Arc<AppState>,
    endpoint: &str,
    start: &Instant,
    response: Response,
) -> Response {
    record_stat(state, endpoint, 500, start).await;
    response
}
