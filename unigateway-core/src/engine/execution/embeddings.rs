use crate::error::GatewayError;
use crate::request::ProxyEmbeddingsRequest;
use crate::response::{CompletedResponse, EmbeddingsResponse, RequestKind};

use super::super::UniGatewayEngine;
use super::fallback::{EndpointAttemptOutput, RequestExecutionParams};
use super::support::execute_embeddings_attempt;

impl UniGatewayEngine {
    /// Executes a stateless vector embeddings extraction.
    pub async fn proxy_embeddings(
        &self,
        request: ProxyEmbeddingsRequest,
        target: crate::pool::ExecutionTarget,
    ) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
        match self
            .execute_with_fallback(
                request,
                target,
                RequestExecutionParams {
                    kind: RequestKind::Embeddings,
                    stream_kind: None,
                    streaming: false,
                },
                None,
                |driver, context, request, timeout| {
                    Box::pin(execute_embeddings_attempt(
                        driver, context, request, timeout,
                    ))
                },
            )
            .await?
        {
            EndpointAttemptOutput::Completed(response) => Ok(*response),
            EndpointAttemptOutput::Streaming(_) => {
                unreachable!("embeddings attempts never produce a streaming session")
            }
        }
    }
}
