use crate::error::GatewayError;
use crate::request::ProxyResponsesRequest;
use crate::response::{ProxySession, RequestKind, ResponsesEvent, ResponsesFinal, StreamKind};

use super::super::UniGatewayEngine;
use super::fallback::{EndpointAttemptOutput, RequestExecutionParams};
use super::support::execute_responses_attempt;

impl UniGatewayEngine {
    /// Dispatches a proxy responses stream request.
    pub async fn proxy_responses(
        &self,
        request: ProxyResponsesRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
        let streaming = request.stream;

        match self
            .execute_with_fallback(
                request,
                target,
                RequestExecutionParams {
                    kind: RequestKind::Responses,
                    stream_kind: Some(StreamKind::Responses),
                    streaming,
                },
                None,
                |driver, context, request, timeout| {
                    Box::pin(execute_responses_attempt(driver, context, request, timeout))
                },
            )
            .await?
        {
            EndpointAttemptOutput::Completed(result) => Ok(ProxySession::Completed(*result)),
            EndpointAttemptOutput::Streaming(streaming) => Ok(ProxySession::Streaming(streaming)),
        }
    }
}
