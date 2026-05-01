use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::pool::EndpointId;
use crate::response::AttemptReport;

/// Comprehensive error type representing all possible failures during request routing, retry, and execution.
#[derive(Debug, Error)]
pub enum GatewayError {
    /// A requested provider pool does not exist in the engine instance.
    #[error("pool not found: {0}")]
    PoolNotFound(String),

    /// Direct execution was requested for a specific endpoint, but it wasn't found.
    #[error("endpoint not found: {0}")]
    EndpointNotFound(String),

    /// The generic inbound request data was semantically invalid or missing essential fields.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The engine was unable to build properly, likely due to missing required configurations.
    #[error("engine build failed: {0}")]
    BuildError(String),

    /// A specific endpoint was skipped due to adaptive concurrency saturation.

    /// All available endpoints for a pool were saturated due to adaptive concurrency.
    #[error("all endpoints saturated for pool: {pool_id:?}")]
    AllEndpointsSaturated {
        /// Optional pool ID that was exhausted.
        pool_id: Option<crate::pool::PoolId>,
    },

    /// A pool was targeted, but no healthy or enabled endpoints exist to service it.
    #[error("no available endpoint for pool: {pool_id:?}")]
    NoAvailableEndpoint {
        /// Optional pool ID that was exhausted.
        pool_id: Option<crate::pool::PoolId>,
    },

    /// The engine exhausted all permitted retries across all allowed endpoints.
    #[error("all attempts failed: {last_error}")]
    AllAttemptsFailed {
        /// Sequence of all attempts made and their recorded outcomes.
        attempts: Vec<AttemptReport>,
        /// The terminal error that caused the final attempt to fall through.
        #[source]
        last_error: Box<GatewayError>,
    },

    /// An upstream provider returned an HTTP-level error (e.g., 400 Bad Request, 429 Rate Limit).
    #[error("upstream http error: {status}")]
    UpstreamHttp {
        /// The raw status code returned by the upstream provider.
        status: u16,
        /// The raw JSON or text body of the upstream error response, if available.
        body: Option<String>,
        /// The remote endpoint ID that produced this error.
        endpoint_id: EndpointId,
    },

    /// An underlying network transport error occurred (timeout, DNS resolution, broken pipe).
    #[error("transport error: {message}")]
    Transport {
        /// The exact inner transport driver message.
        message: String,
        /// The remote endpoint ID affected, if applicable.
        endpoint_id: Option<EndpointId>,
    },

    /// A streaming response connection crashed midway through transmission.
    #[error("stream aborted: {message}")]
    StreamAborted {
        /// Reason for abort.
        message: String,
        /// The remote endpoint ID providing the stream.
        endpoint_id: EndpointId,
    },

    /// Features not yet supported by standard drivers.
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
}

/// Stable error buckets that hosts can map into neutral observability or scoring systems.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayErrorKind {
    Timeout,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    ConnectionFailure,
    InvalidResponse,
    CancelledByClient,
    StreamAborted,
    Other,
}

impl GatewayError {
    /// Convenience constructor for NotImplemented errors.
    pub fn not_implemented(feature: &'static str) -> Self {
        Self::NotImplemented(feature)
    }

    /// Retrieve the comprehensive trail of attempted requests, if this error is an exhaustion failure.
    pub fn attempts(&self) -> Option<&[AttemptReport]> {
        match self {
            Self::AllAttemptsFailed { attempts, .. } => Some(attempts),
            _ => None,
        }
    }

    /// Return the root inner error causing this error boundary. Useful to unwrap nested `AllAttemptsFailed` blocks.
    pub fn terminal_error(&self) -> &Self {
        match self {
            Self::AllAttemptsFailed { last_error, .. } => last_error.terminal_error(),
            _ => self,
        }
    }

    /// Resolves the upstream HTTP status code, if any.
    pub fn status_code(&self) -> Option<u16> {
        match self.terminal_error() {
            Self::UpstreamHttp { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Classifies the terminal error into a stable, host-consumable bucket.
    pub fn kind(&self) -> GatewayErrorKind {
        match self.terminal_error() {
            Self::UpstreamHttp { status, .. } if *status == 429 => GatewayErrorKind::RateLimited,
            Self::UpstreamHttp { status, .. } if (500..=599).contains(status) => {
                GatewayErrorKind::Upstream5xx
            }
            Self::UpstreamHttp { status, .. } if (400..=499).contains(status) => {
                GatewayErrorKind::Upstream4xx
            }
            Self::UpstreamHttp { .. } => GatewayErrorKind::Other,
            Self::Transport { message, .. } => {
                let normalized = message.to_ascii_lowercase();
                if normalized.contains("timed out") {
                    GatewayErrorKind::Timeout
                } else if normalized.contains("cancelled by client")
                    || normalized.contains("canceled by client")
                {
                    GatewayErrorKind::CancelledByClient
                } else {
                    GatewayErrorKind::ConnectionFailure
                }
            }
            Self::StreamAborted { message, .. } => {
                let normalized = message.to_ascii_lowercase();
                if normalized.contains("cancelled by client")
                    || normalized.contains("canceled by client")
                {
                    GatewayErrorKind::CancelledByClient
                } else {
                    GatewayErrorKind::StreamAborted
                }
            }
            Self::InvalidRequest(_) => GatewayErrorKind::InvalidResponse,
            Self::BuildError(_) | Self::PoolNotFound(_) | Self::EndpointNotFound(_) => {
                GatewayErrorKind::Other
            }
            Self::AllEndpointsSaturated { .. }
            | Self::NoAvailableEndpoint { .. }
            | Self::AllAttemptsFailed { .. }
            | Self::NotImplemented(_) => GatewayErrorKind::Other,
        }
    }
}
