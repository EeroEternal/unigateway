use http::StatusCode;

use crate::error::{HostError, PoolLookupErrorKind};

pub fn status_for_host_error(error: &HostError) -> StatusCode {
    match error {
        HostError::InvalidDispatchRequest { .. }
        | HostError::Targeting(_)
        | HostError::CoreInvalidRequest(_)
        | HostError::CorePoolNotFound(_)
        | HostError::CoreEndpointNotFound(_) => StatusCode::BAD_REQUEST,
        HostError::PoolLookup(error) => match error.kind() {
            PoolLookupErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            PoolLookupErrorKind::Unavailable | PoolLookupErrorKind::Other => {
                StatusCode::BAD_GATEWAY
            }
        },
        HostError::CoreBuild(_) | HostError::CoreNotImplemented(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        HostError::CoreAllEndpointsSaturated { .. } | HostError::CoreNoAvailableEndpoint { .. } => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        HostError::CoreUpstreamHttp { status, .. } => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
        }
        HostError::CoreTransport { .. } | HostError::CoreStreamAborted { .. } => {
            StatusCode::BAD_GATEWAY
        }
        HostError::CoreAllAttemptsFailed { last_error, .. } => status_for_host_error(last_error),
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use unigateway_core::GatewayError;

    use crate::error::HostError;

    use super::status_for_host_error;

    #[test]
    fn host_error_status_distinguishes_target_mismatch() {
        assert_eq!(
            status_for_host_error(&HostError::targeting(
                "no provider matches target 'deepseek'"
            )),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for_host_error(&HostError::core(GatewayError::UpstreamHttp {
                status: 502,
                body: Some("upstream request failed".to_string()),
                endpoint_id: "ep-openai-main".to_string(),
            })),
            StatusCode::BAD_GATEWAY
        );
    }
}
