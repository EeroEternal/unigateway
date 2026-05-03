use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
};
use serde_json::Value;
use tokio::net::TcpListener;

use unigateway_sdk::core::retry::LoadBalancingStrategy;
use unigateway_sdk::core::{
    Endpoint, ModelPolicy, ProviderKind, SecretString, UniGatewayEngine,
    pool::{ExecutionTarget, ProviderPool},
};
use unigateway_sdk::protocol::openai_payload_to_chat_request;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::default())
        .unwrap();

    let base_url =
        env::var("UPSTREAM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com".to_string());
    let api_key = env::var("UPSTREAM_API_KEY").unwrap_or_else(|_| "sk-".to_string());
    let model = env::var("UPSTREAM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let bind_addr: SocketAddr = env::var("BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3210".to_string())
        .parse()?;

    let engine = UniGatewayEngine::builder()
        .with_builtin_http_drivers()
        .build()?;

    let endpoint = Endpoint {
        endpoint_id: "ep-1".to_string(),
        provider_name: Some("openai-main".to_string()),
        source_endpoint_id: Some("openai-main".to_string()),
        provider_family: Some("openai".to_string()),
        provider_kind: ProviderKind::OpenAiCompatible,
        driver_id: "openai-compatible".to_string(),
        base_url,
        api_key: SecretString::new(api_key),
        model_policy: ModelPolicy {
            default_model: Some(model.clone()),
            model_mapping: HashMap::new(),
        },
        enabled: true,
        metadata: HashMap::new(),
    };

    let pool = ProviderPool {
        pool_id: "test-pool".to_string(),
        endpoints: vec![endpoint],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: Default::default(),
        metadata: HashMap::new(),
    };

    engine.upsert_pool(pool).await?;

    let engine = Arc::new(engine);
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(engine);

    let listener = TcpListener::bind(bind_addr).await?;
    println!("Example server listening on http://{}", bind_addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn chat_completions(
    State(engine): State<Arc<UniGatewayEngine>>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let default_model = env::var("UPSTREAM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let request = openai_payload_to_chat_request(&payload, &default_model)?;

    let extra_fields = request.extra.clone();
    if !extra_fields.is_empty() {
        tracing::info!("Extra fields in request: {:?}", extra_fields.keys());
    }

    let target = ExecutionTarget::Pool {
        pool_id: "test-pool".to_string(),
    };
    let session = engine.proxy_chat(request, target).await?;

    let is_streaming = matches!(
        session,
        unigateway_sdk::core::response::ProxySession::Streaming(_)
    );

    Ok(Json(serde_json::json!({
        "status": "success",
        "extra_fields_received": extra_fields,
        "streaming": is_streaming,
    })))
}

#[derive(Debug)]
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": self.0.to_string()
            })),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
