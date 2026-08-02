use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::pool::ProviderKind;
use crate::protocol::openai::OpenAiCompatibleDriver;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal,
};
use crate::transport::HttpTransport;

pub mod backend;

#[cfg(feature = "sglang-lite-grpc")]
mod grpc;

pub use backend::{
    BACKEND_MODE_KEY, SUBPROCESS_ARGS_KEY, SUBPROCESS_COMMAND_KEY, SUBPROCESS_HEALTH_PATH_KEY,
    SUBPROCESS_STARTUP_TIMEOUT_MS_KEY, SglangLiteBackend, SglangLiteSubprocess,
    SglangLiteSubprocessConfig,
};

pub const DRIVER_ID: &str = "sglang-lite";

/// Metadata key for the local sglang-lite model path.
pub const MODEL_PATH_KEY: &str = "unigateway.sglang_lite.model_path";
/// Metadata key for the device to run the sglang-lite engine on (e.g. `cuda` or `cpu`).
pub const DEVICE_KEY: &str = "unigateway.sglang_lite.device";
/// Metadata key for the maximum batch size used by the sglang-lite scheduler.
pub const MAX_BATCH_SIZE_KEY: &str = "unigateway.sglang_lite.max_batch_size";
/// Metadata key for the Python environment / interpreter path when using direct import modes.
pub const PYTHON_ENV_KEY: &str = "unigateway.sglang_lite.python_env";

/// Driver-specific options parsed from `DriverEndpointContext.metadata`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SglangLiteOptions {
    /// Local model path or identifier passed to the sglang-lite engine.
    pub model_path: Option<String>,
    /// Target device (e.g. `cuda`, `cpu`).
    pub device: Option<String>,
    /// Maximum batch size for the scheduler.
    pub max_batch_size: Option<String>,
    /// Python environment / interpreter path for direct import modes.
    pub python_env: Option<String>,
}

impl SglangLiteOptions {
    /// Parse options from endpoint metadata.
    pub fn from_metadata(metadata: &std::collections::HashMap<String, String>) -> Self {
        Self {
            model_path: metadata.get(MODEL_PATH_KEY).cloned(),
            device: metadata.get(DEVICE_KEY).cloned(),
            max_batch_size: metadata.get(MAX_BATCH_SIZE_KEY).cloned(),
            python_env: metadata.get(PYTHON_ENV_KEY).cloned(),
        }
    }
}

/// sglang-lite driver.
///
/// In the initial HTTP transport mode this driver reuses the OpenAI-compatible request/response
/// translation and streaming logic, because sglang-lite exposes an OpenAI-compatible
/// `/v1/chat/completions` surface. It carries a distinct `ProviderKind::SglangLite` identity and
/// driver-specific capability defaults (e.g. prefix caching) so that upper layers can recognize
/// local MoE backends and collect the relevant metrics without leaking sglang-lite details into
/// core abstractions.
pub struct SglangLiteDriver {
    transport: Arc<dyn HttpTransport>,
    subprocess: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
}

impl SglangLiteDriver {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self {
            transport,
            subprocess: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }
}

impl ProviderDriver for SglangLiteDriver {
    fn driver_id(&self) -> &str {
        DRIVER_ID
    }

    fn provider_kind(&self) -> ProviderKind {
        ProviderKind::SglangLite
    }

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>
    {
        let transport = self.transport.clone();
        let subprocess = self.subprocess.clone();

        Box::pin(async move {
            let backend = SglangLiteBackend::from_metadata(&endpoint.metadata, &endpoint.base_url)?;

            if let SglangLiteBackend::Subprocess(config) = backend {
                let mut guard = subprocess.lock().await;
                if guard.is_none() {
                    let child = SglangLiteSubprocess::new(config)
                        .start(transport.as_ref())
                        .await?;
                    *guard = Some(child);
                }
            } else if backend == SglangLiteBackend::Grpc {
                // gRPC support is defined in sglang-lite/proto/sglang_lite.proto
                // and docs/sglang-lite-grpc-spec.md (P2 priority).
                // See SglangLiteBackend::Grpc for the confirmed contract.
                #[cfg(feature = "sglang-lite-grpc")]
                {
                    // Support subprocess start for gRPC if the subprocess.* metadata keys are present
                    let mut guard = subprocess.lock().await;
                    if guard.is_none()
                        && let Ok(sub_cfg) = backend::SglangLiteSubprocessConfig::from_metadata(
                            &endpoint.metadata,
                            &endpoint.base_url,
                        )
                    {
                        let child = grpc::spawn_and_wait_grpc_health(sub_cfg).await?;
                        *guard = Some(child);
                    }
                    return grpc::execute_chat_grpc(endpoint, request).await;
                }
                #[cfg(not(feature = "sglang-lite-grpc"))]
                {
                    return Err(GatewayError::not_implemented(
                        "sglang-lite grpc (compile with feature \"sglang-lite-grpc\" to enable the client skeleton)",
                    ));
                }
            }

            OpenAiCompatibleDriver::new(transport)
                .execute_chat(endpoint, request)
                .await
        })
    }

    fn execute_responses(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>
    {
        Box::pin(async { Err(GatewayError::not_implemented("sglang-lite responses")) })
    }

    fn execute_embeddings(
        &self,
        _endpoint: DriverEndpointContext,
        _request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>> {
        Box::pin(async { Err(GatewayError::not_implemented("sglang-lite embeddings")) })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use futures_util::future::BoxFuture;
    use serde_json::json;

    use super::{
        DEVICE_KEY, MAX_BATCH_SIZE_KEY, MODEL_PATH_KEY, PYTHON_ENV_KEY, SglangLiteDriver,
        SglangLiteOptions,
    };
    use crate::capabilities::EndpointCapabilities;
    use crate::drivers::{DriverEndpointContext, ProviderDriver};
    use crate::pool::{ModelPolicy, ProviderKind, SecretString};
    use crate::request::{Message, MessageRole, ProxyChatRequest};
    use crate::response::ProxySession;
    use crate::transport::{HttpTransport, TransportRequest, TransportResponse};

    struct MockTransport {
        seen: Arc<Mutex<Vec<TransportRequest>>>,
    }

    impl HttpTransport for MockTransport {
        fn send(
            &self,
            request: TransportRequest,
        ) -> BoxFuture<'static, Result<TransportResponse, crate::GatewayError>> {
            let seen = self.seen.clone();
            Box::pin(async move {
                seen.lock().expect("seen lock").push(request);
                Ok(TransportResponse {
                    status: 200,
                    headers: HashMap::new(),
                    body: serde_json::to_vec(&json!({
                        "id": "chatcmpl-sglang",
                        "object": "chat.completion",
                        "model": "test-moe",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "hello from sglang" },
                            "finish_reason": "stop"
                        }],
                        "usage": { "prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5, "cache_hit_tokens": 1 }
                    }))
                    .expect("body"),
                })
            })
        }

        fn send_stream(
            &self,
            _request: TransportRequest,
        ) -> BoxFuture<
            'static,
            Result<crate::transport::StreamingTransportResponse, crate::GatewayError>,
        > {
            Box::pin(async {
                Err(crate::GatewayError::not_implemented(
                    "sglang-lite mock stream",
                ))
            })
        }
    }

    fn endpoint() -> DriverEndpointContext {
        DriverEndpointContext {
            endpoint_id: "ep-sglang".to_string(),
            provider_kind: ProviderKind::SglangLite,
            base_url: "http://localhost:8000/v1/".to_string(),
            api_key: SecretString::new(""),
            model_policy: ModelPolicy {
                default_model: Some("local-moe".to_string()),
                model_mapping: HashMap::new(),
            },
            capabilities: EndpointCapabilities::default(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn driver_reports_sglang_identity() {
        let transport = Arc::new(MockTransport {
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let driver = SglangLiteDriver::new(transport);

        assert_eq!(driver.driver_id(), "sglang-lite");
        assert_eq!(driver.provider_kind(), ProviderKind::SglangLite);
    }

    #[tokio::test]
    async fn execute_chat_delegates_to_openai_compatible_http() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(MockTransport { seen: seen.clone() });
        let driver = SglangLiteDriver::new(transport);
        let request = ProxyChatRequest {
            model: "local-moe".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };

        let session = driver
            .execute_chat(endpoint(), request)
            .await
            .expect("chat succeeds");
        let ProxySession::Completed(completed) = session else {
            panic!("expected completed response");
        };

        assert_eq!(
            completed.response.output_text.as_deref(),
            Some("hello from sglang")
        );
        assert_eq!(completed.report.selected_provider, ProviderKind::SglangLite);
        assert_eq!(
            completed
                .report
                .usage
                .and_then(|usage| usage.cache_hit_tokens),
            Some(1),
            "cache_hit_tokens should be parsed from sglang-lite response"
        );

        let requests = seen.lock().expect("seen lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "http://localhost:8000/v1/chat/completions");
        assert!(
            !requests[0].headers.contains_key("authorization"),
            "local backend should not send an empty authorization header"
        );
    }

    #[test]
    fn options_parse_sglang_lite_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert(MODEL_PATH_KEY.to_string(), "/path/to/moe".to_string());
        metadata.insert(DEVICE_KEY.to_string(), "cuda".to_string());
        metadata.insert(MAX_BATCH_SIZE_KEY.to_string(), "16".to_string());
        metadata.insert(PYTHON_ENV_KEY.to_string(), "/opt/python".to_string());

        let options = SglangLiteOptions::from_metadata(&metadata);
        assert_eq!(options.model_path.as_deref(), Some("/path/to/moe"));
        assert_eq!(options.device.as_deref(), Some("cuda"));
        assert_eq!(options.max_batch_size.as_deref(), Some("16"));
        assert_eq!(options.python_env.as_deref(), Some("/opt/python"));
    }

    #[tokio::test]
    async fn execute_responses_is_not_implemented() {
        let transport = Arc::new(MockTransport {
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let driver = SglangLiteDriver::new(transport);
        let request = crate::request::ProxyResponsesRequest {
            model: "local-moe".to_string(),
            input: None,
            instructions: None,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            stream: false,
            tools: None,
            tool_choice: None,
            reasoning: None,
            previous_response_id: None,
            request_metadata: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };

        let result = driver.execute_responses(endpoint(), request).await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("expected responses to be not implemented"),
        };
        assert!(
            error.to_string().contains("not implemented"),
            "responses should be reported as not implemented"
        );
    }

    #[tokio::test]
    #[cfg(not(feature = "sglang-lite-grpc"))]
    async fn execute_chat_returns_not_implemented_for_grpc_backend() {
        let transport = Arc::new(MockTransport {
            seen: Arc::new(Mutex::new(Vec::new())),
        });
        let driver = SglangLiteDriver::new(transport);
        let mut endpoint = endpoint();
        endpoint
            .metadata
            .insert(super::BACKEND_MODE_KEY.to_string(), "grpc".to_string());

        let request = ProxyChatRequest {
            model: "local-moe".to_string(),
            messages: vec![Message::text(MessageRole::User, "hello")],
            temperature: None,
            top_p: None,
            top_k: None,
            max_tokens: None,
            stop_sequences: None,
            stream: false,
            system: None,
            tools: None,
            tool_choice: None,
            raw_messages: None,
            extra: HashMap::new(),
            metadata: HashMap::new(),
        };

        let result = driver.execute_chat(endpoint, request).await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("expected grpc backend to be not implemented"),
        };
        assert!(
            error.to_string().contains("not implemented"),
            "grpc backend should be reported as not implemented (see sglang-lite proto + spec)"
        );
    }
}
