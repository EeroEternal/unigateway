use thiserror::Error;

use crate::pool::EndpointId;
use crate::response::AttemptReport;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("pool not found: {0}")]
    PoolNotFound(String),

    #[error("endpoint not found: {0}")]
    EndpointNotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("no available endpoint")]
    NoAvailableEndpoint,

    #[error("all attempts failed")]
    AllAttemptsFailed { attempts: Vec<AttemptReport> },

    #[error("upstream http error: {status}")]
    UpstreamHttp {
        status: u16,
        body: Option<String>,
        endpoint_id: EndpointId,
    },

    #[error("transport error: {message}")]
    Transport {
        message: String,
        endpoint_id: Option<EndpointId>,
    },

    #[error("stream aborted: {message}")]
    StreamAborted {
        message: String,
        endpoint_id: EndpointId,
    },

    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
}

impl GatewayError {
    pub fn not_implemented(feature: &'static str) -> Self {
        Self::NotImplemented(feature)
    }
}
