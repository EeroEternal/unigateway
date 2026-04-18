use std::sync::Arc;
use std::time::SystemTime;

use futures_util::future::BoxFuture;

use crate::drivers::{DriverEndpointContext, ProviderDriver};
use crate::error::GatewayError;
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal,
};
use crate::transport::HttpTransport;

use super::DRIVER_ID;
use super::parsing::{parse_chat_response, parse_embeddings_response, parse_responses_response};
use super::requests::{build_chat_request, build_embeddings_request, build_responses_request};
use super::streaming::{start_chat_stream, start_responses_stream};

pub struct OpenAiCompatibleDriver {
    transport: Arc<dyn HttpTransport>,
}

impl OpenAiCompatibleDriver {
    pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
        Self { transport }
    }
}

impl ProviderDriver for OpenAiCompatibleDriver {
    fn driver_id(&self) -> &str {
        DRIVER_ID
    }

    fn provider_kind(&self) -> crate::ProviderKind {
        crate::ProviderKind::OpenAiCompatible
    }

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>
    {
        let transport = self.transport.clone();

        Box::pin(async move {
            if request.stream {
                return start_chat_stream(transport, endpoint, request).await;
            }

            let started_at = SystemTime::now();
            let transport_request = build_chat_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_chat_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(ProxySession::Completed(CompletedResponse {
                response: response_body,
                report: super::super::build_request_report(
                    &endpoint,
                    started_at,
                    finished_at,
                    usage,
                    None,
                ),
            }))
        })
    }

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyResponsesRequest,
    ) -> BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>
    {
        let transport = self.transport.clone();

        Box::pin(async move {
            if request.stream {
                return start_responses_stream(transport, endpoint, request).await;
            }

            let started_at = SystemTime::now();
            let transport_request = build_responses_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_responses_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(ProxySession::Completed(CompletedResponse {
                response: response_body,
                report: super::super::build_request_report(
                    &endpoint,
                    started_at,
                    finished_at,
                    usage,
                    None,
                ),
            }))
        })
    }

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyEmbeddingsRequest,
    ) -> BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>> {
        let transport = self.transport.clone();

        Box::pin(async move {
            let started_at = SystemTime::now();
            let transport_request = build_embeddings_request(&endpoint, &request)?;
            let response = transport.send(transport_request).await?;
            if !(200..300).contains(&response.status) {
                return Err(GatewayError::UpstreamHttp {
                    status: response.status,
                    body: String::from_utf8(response.body).ok(),
                    endpoint_id: endpoint.endpoint_id,
                });
            }

            let (response_body, usage) = parse_embeddings_response(&response.body)?;
            let finished_at = SystemTime::now();

            Ok(CompletedResponse {
                response: response_body,
                report: super::super::build_request_report(
                    &endpoint,
                    started_at,
                    finished_at,
                    usage,
                    None,
                ),
            })
        })
    }
}
