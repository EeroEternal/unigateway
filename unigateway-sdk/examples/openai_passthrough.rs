//! Production-style OpenAI Chat Completions passthrough using host dispatch + protocol render.
//!
//! Environment:
//! - `UPSTREAM_BASE_URL` — OpenAI-compatible base (default `https://api.openai.com/v1`)
//! - `UPSTREAM_API_KEY` — bearer token (default `sk-`)
//! - `UPSTREAM_MODEL` — default model when body omits `model`
//! - `BIND_ADDR` — listen address (default `127.0.0.1:3210`)
//!
//! Smoke test (non-streaming):
//! ```text
//! cargo run -p unigateway-sdk --example openai_passthrough
//! curl -s http://127.0.0.1:3210/v1/chat/completions \
//!   -H 'content-type: application/json' \
//!   -d '{"messages":[{"role":"user","content":"hi"}],"stream":false}'
//! ```

use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpListener;
use unigateway_sdk::core::retry::LoadBalancingStrategy;
use unigateway_sdk::core::{
    Endpoint, EndpointCapabilities, ModelPolicy, ProviderKind, SecretString, UniGatewayEngine,
    pool::ProviderPool,
};
use unigateway_sdk::host::{
    HostContext, HostDispatchOutcome, HostDispatchTarget, HostFuture, HostProtocol, HostRequest,
    PoolHost, PoolLookupOutcome, PoolLookupResult, dispatch_request,
};
use unigateway_sdk::protocol::{
    ProtocolHttpResponse, ProtocolResponseBody, openai_payload_to_chat_request,
};

#[derive(Clone)]
struct AppState {
    engine: Arc<UniGatewayEngine>,
    pool: ProviderPool,
    default_model: String,
    pool_host: Arc<StaticPoolHost>,
}

struct StaticPoolHost {
    pool: ProviderPool,
}

impl PoolHost for StaticPoolHost {
    fn pool_for_service<'a>(
        &'a self,
        _service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        let pool = self.pool.clone();
        Box::pin(async move { Ok(PoolLookupOutcome::found(pool)) })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(tracing_subscriber::FmtSubscriber::default()).unwrap();

    let base_url =
        env::var("UPSTREAM_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let api_key = env::var("UPSTREAM_API_KEY").unwrap_or_else(|_| "sk-".to_string());
    let default_model = env::var("UPSTREAM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
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
            default_model: Some(default_model.clone()),
            model_mapping: HashMap::new(),
        },
        enabled: true,
        max_concurrency: None,
        capabilities: EndpointCapabilities::default(),
        metadata: HashMap::new(),
        forward_metadata_as_headers: None,
    };

    let pool = ProviderPool {
        pool_id: "passthrough-pool".to_string(),
        endpoints: vec![endpoint],
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy: Default::default(),
        metadata: HashMap::new(),
        forward_metadata_as_headers: None,
    };

    engine.upsert_pool(pool.clone()).await?;

    let state = AppState {
        engine: Arc::new(engine),
        pool_host: Arc::new(StaticPoolHost { pool: pool.clone() }),
        pool,
        default_model,
    };

    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let listener = TcpListener::bind(bind_addr).await?;
    println!("OpenAI passthrough listening on http://{bind_addr}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn chat_completions(
    State(state): State<AppState>,
    body: axum::Json<Value>,
) -> Result<Response, AppError> {
    let request =
        openai_payload_to_chat_request(&body, &state.default_model).map_err(AppError::parse)?;

    let context = HostContext::from_parts(&state.engine, state.pool_host.as_ref());
    let outcome = dispatch_request(
        &context,
        HostDispatchTarget::Pool(state.pool.clone()),
        HostProtocol::OpenAiChat,
        None,
        HostRequest::Chat(request),
    )
    .await
    .map_err(AppError::host)?;

    let HostDispatchOutcome::Response(response) = outcome else {
        return Err(AppError::pool_not_found());
    };

    Ok(protocol_response_to_axum(response))
}

fn protocol_response_to_axum(response: ProtocolHttpResponse) -> Response {
    let (status, body) = response.into_parts();
    match body {
        ProtocolResponseBody::Json(value) => (status, axum::Json(value)).into_response(),
        ProtocolResponseBody::ServerSentEvents(stream) => {
            let body_stream = stream.map(|result| result);
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from_stream(body_stream))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn parse(error: impl Into<anyhow::Error>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.into().to_string(),
        }
    }

    fn host(error: unigateway_sdk::host::HostError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        }
    }

    fn pool_not_found() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "provider pool not found".to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            axum::Json(serde_json::json!({
                "error": {
                    "message": self.message,
                    "type": "gateway_error"
                }
            })),
        )
            .into_response()
    }
}
