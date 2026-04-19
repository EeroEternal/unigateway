use std::future::Future;

use anyhow::Result;
use http::StatusCode;
use serde_json::json;
use unigateway_protocol::RuntimeHttpResponse;

use crate::status::status_for_core_error;

pub type HostResponseResult = Result<RuntimeHttpResponse, RuntimeHttpResponse>;

pub async fn resolve_core_only_host_flow<CoreFuture>(
    core_attempt: CoreFuture,
    unavailable_message: &str,
) -> HostResponseResult
where
    CoreFuture: Future<Output = anyhow::Result<Option<RuntimeHttpResponse>>>,
{
    match core_attempt.await {
        Ok(Some(response)) => Ok(response),
        Ok(None) => Err(error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            unavailable_message,
        )),
        Err(error) => Err(core_error_response(&error)),
    }
}

pub fn missing_upstream_api_key_response() -> RuntimeHttpResponse {
    error_json(StatusCode::BAD_REQUEST, "missing upstream api key")
}

fn core_error_response(error: &anyhow::Error) -> RuntimeHttpResponse {
    error_json(
        status_for_core_error(error),
        &format!("core execution error: {error:#}"),
    )
}

fn error_json(status: StatusCode, message: &str) -> RuntimeHttpResponse {
    RuntimeHttpResponse::json(status, json!({"error": {"message": message}}))
}
