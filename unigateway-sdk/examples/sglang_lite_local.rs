use std::collections::HashMap;

use unigateway_sdk::core::{
    BACKEND_MODE_KEY, DEVICE_KEY, DRIVER_ID, Endpoint, EndpointCapabilities, MAX_BATCH_SIZE_KEY,
    MODEL_PATH_KEY, ModelPolicy, PYTHON_ENV_KEY, ProviderKind, SUBPROCESS_ARGS_KEY,
    SUBPROCESS_COMMAND_KEY, SUBPROCESS_HEALTH_PATH_KEY, SUBPROCESS_STARTUP_TIMEOUT_MS_KEY,
    SecretString, UniGatewayEngine,
    pool::{ExecutionTarget, ProviderPool},
    response::ProxySession,
    retry::LoadBalancingStrategy,
};
use unigateway_sdk::protocol::openai_payload_to_chat_request;

/// Minimal example: configure a local sglang-lite backend via UniGateway.
///
/// By default this connects to an already-running sglang-lite HTTP server.
/// Set `SGLANG_LITE_BACKEND_MODE=subprocess` and `SGLANG_LITE_SUBPROCESS_COMMAND`
/// to have UniGateway spawn the engine as a child process instead.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("SGLANG_LITE_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let model = std::env::var("SGLANG_LITE_MODEL").unwrap_or_else(|_| "local-moe".to_string());
    let model_path = std::env::var("SGLANG_LITE_MODEL_PATH")
        .unwrap_or_else(|_| "/path/to/local-moe".to_string());
    let device = std::env::var("SGLANG_LITE_DEVICE").unwrap_or_else(|_| "cuda".to_string());
    let backend_mode =
        std::env::var("SGLANG_LITE_BACKEND_MODE").unwrap_or_else(|_| "http".to_string());

    let engine = UniGatewayEngine::builder()
        .with_builtin_http_drivers()
        .build()?;

    let mut metadata = HashMap::new();
    metadata.insert(MODEL_PATH_KEY.to_string(), model_path);
    metadata.insert(DEVICE_KEY.to_string(), device);
    metadata.insert(MAX_BATCH_SIZE_KEY.to_string(), "16".to_string());
    metadata.insert(PYTHON_ENV_KEY.to_string(), "python3".to_string());
    metadata.insert(BACKEND_MODE_KEY.to_string(), backend_mode);

    if let Ok(command) = std::env::var("SGLANG_LITE_SUBPROCESS_COMMAND") {
        metadata.insert(SUBPROCESS_COMMAND_KEY.to_string(), command);
        if let Ok(args) = std::env::var("SGLANG_LITE_SUBPROCESS_ARGS") {
            metadata.insert(SUBPROCESS_ARGS_KEY.to_string(), args);
        }
        metadata.insert(
            SUBPROCESS_STARTUP_TIMEOUT_MS_KEY.to_string(),
            std::env::var("SGLANG_LITE_SUBPROCESS_STARTUP_TIMEOUT_MS")
                .unwrap_or_else(|_| "30000".to_string()),
        );
        metadata.insert(
            SUBPROCESS_HEALTH_PATH_KEY.to_string(),
            std::env::var("SGLANG_LITE_SUBPROCESS_HEALTH_PATH")
                .unwrap_or_else(|_| "health".to_string()),
        );
    }

    let endpoint = Endpoint {
        endpoint_id: "sglang-local".to_string(),
        provider_name: Some("sglang-local".to_string()),
        source_endpoint_id: Some("sglang-local".to_string()),
        provider_family: Some("sglang-lite".to_string()),
        provider_kind: ProviderKind::SglangLite,
        driver_id: DRIVER_ID.to_string(),
        base_url,
        api_key: SecretString::new(""),
        model_policy: ModelPolicy {
            default_model: Some(model.clone()),
            model_mapping: HashMap::new(),
        },
        enabled: true,
        max_concurrency: None,
        capabilities: EndpointCapabilities::default(),
        metadata,
    };

    let pool = ProviderPool {
        pool_id: "sglang-pool".to_string(),
        endpoints: vec![endpoint],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: Default::default(),
        metadata: HashMap::new(),
    };

    engine.upsert_pool(pool).await?;

    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hello, sglang-lite!"}],
        "stream": false,
    });
    let request = openai_payload_to_chat_request(&payload, &model)?;

    let target = ExecutionTarget::Pool {
        pool_id: "sglang-pool".to_string(),
    };
    let session = engine.proxy_chat(request, target).await?;

    match session {
        ProxySession::Completed(result) => {
            println!("Completion: {:?}", result.response.output_text);
        }
        ProxySession::Streaming(_streaming) => {
            println!("Streaming response started");
        }
    }

    Ok(())
}
