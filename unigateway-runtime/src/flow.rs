use std::future::Future;

use anyhow::Result;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::host::RuntimeConfig;
use crate::status::{status_for_core_error, status_for_legacy_error};

pub struct EnvRuntimeConfig<'a> {
    pub base_url: &'a str,
    pub api_key: String,
}

pub type RuntimeResponseResult = Result<Response, Response>;

pub async fn resolve_authenticated_runtime_flow<CoreFuture, LegacyFuture>(
    core_attempt: CoreFuture,
    legacy_attempt: LegacyFuture,
) -> RuntimeResponseResult
where
    CoreFuture: Future<Output = anyhow::Result<Option<Response>>>,
    LegacyFuture: Future<Output = anyhow::Result<Response>>,
{
    match core_attempt.await {
        Ok(Some(response)) => return Ok(response),
        Ok(None) => {}
        Err(error) => return Err(core_error_response(&error)),
    }

    match legacy_attempt.await {
        Ok(response) => Ok(response),
        Err(error) => Err(legacy_error_response(&error)),
    }
}

pub async fn resolve_env_runtime_flow<CoreFuture, LegacyFuture>(
    core_attempt: CoreFuture,
    legacy_attempt: LegacyFuture,
) -> RuntimeResponseResult
where
    CoreFuture: Future<Output = anyhow::Result<Option<Response>>>,
    LegacyFuture: Future<Output = anyhow::Result<Response>>,
{
    match core_attempt.await {
        Ok(Some(response)) => return Ok(response),
        Ok(None) => {}
        Err(error) => return Err(core_error_response(&error)),
    }

    match legacy_attempt.await {
        Ok(response) => Ok(response),
        Err(error) => Err(upstream_error_response(&error)),
    }
}

pub async fn resolve_core_only_runtime_flow<CoreFuture>(
    core_attempt: CoreFuture,
    unavailable_message: &str,
) -> RuntimeResponseResult
where
    CoreFuture: Future<Output = anyhow::Result<Option<Response>>>,
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

pub fn fallback_api_key(token: &str, env_key: &str) -> String {
    if !token.is_empty() {
        token.to_string()
    } else {
        env_key.to_string()
    }
}

pub fn prepare_env_config<'a>(
    token: &str,
    env_key: &str,
    base_url: &'a str,
) -> Option<EnvRuntimeConfig<'a>> {
    let api_key = fallback_api_key(token, env_key);

    if api_key.is_empty() {
        None
    } else {
        Some(EnvRuntimeConfig { base_url, api_key })
    }
}

pub fn prepare_openai_env_config<'a>(
    token: &str,
    config: RuntimeConfig<'a>,
) -> Option<EnvRuntimeConfig<'a>> {
    prepare_env_config(token, config.openai_api_key, config.openai_base_url)
}

pub fn prepare_anthropic_env_config<'a>(
    token: &str,
    config: RuntimeConfig<'a>,
) -> Option<EnvRuntimeConfig<'a>> {
    prepare_env_config(token, config.anthropic_api_key, config.anthropic_base_url)
}

pub fn missing_upstream_api_key_response() -> Response {
    error_json(StatusCode::BAD_REQUEST, "missing upstream api key")
}

fn core_error_response(error: &anyhow::Error) -> Response {
    error_json(
        status_for_core_error(error),
        &format!("core execution error: {error:#}"),
    )
}

fn legacy_error_response(error: &anyhow::Error) -> Response {
    error_json(
        status_for_legacy_error(error),
        &format!("legacy execution error: {error:#}"),
    )
}

fn upstream_error_response(error: &anyhow::Error) -> Response {
    error_json(
        StatusCode::BAD_GATEWAY,
        &format!("upstream error: {error:#}"),
    )
}

fn error_json(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": {"message": message}}))).into_response()
}

#[cfg(test)]
mod tests {
    use crate::host::RuntimeConfig;

    use super::{fallback_api_key, prepare_anthropic_env_config, prepare_openai_env_config};

    #[test]
    fn fallback_api_key_prefers_request_token() {
        assert_eq!(fallback_api_key("sk-live", "sk-env"), "sk-live");
        assert_eq!(fallback_api_key("", "sk-env"), "sk-env");
    }

    #[test]
    fn prepare_openai_env_config_uses_openai_fields() {
        let config = RuntimeConfig {
            openai_base_url: "https://api.openai.test",
            openai_api_key: "sk-openai",
            openai_model: "gpt-test",
            anthropic_base_url: "https://api.anthropic.test",
            anthropic_api_key: "sk-anthropic",
            anthropic_model: "claude-test",
        };

        let env = prepare_openai_env_config("", config).expect("env config");

        assert_eq!(env.base_url, "https://api.openai.test");
        assert_eq!(env.api_key, "sk-openai");
    }

    #[test]
    fn prepare_anthropic_env_config_uses_anthropic_fields() {
        let config = RuntimeConfig {
            openai_base_url: "https://api.openai.test",
            openai_api_key: "sk-openai",
            openai_model: "gpt-test",
            anthropic_base_url: "https://api.anthropic.test",
            anthropic_api_key: "sk-anthropic",
            anthropic_model: "claude-test",
        };

        let env = prepare_anthropic_env_config("", config).expect("env config");

        assert_eq!(env.base_url, "https://api.anthropic.test");
        assert_eq!(env.api_key, "sk-anthropic");
    }
}
