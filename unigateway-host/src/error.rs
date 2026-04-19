use std::error::Error;
use std::fmt;

use anyhow::Error as AnyhowError;
use unigateway_core::{AttemptReport, GatewayError};

pub type HostResult<T> = std::result::Result<T, HostError>;
pub type PoolLookupResult<T> = std::result::Result<T, PoolLookupError>;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolLookupErrorKind {
    Unavailable,
    Timeout,
    Other,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum PoolLookupError {
    #[non_exhaustive]
    Unavailable {
        message: String,
    },
    #[non_exhaustive]
    Timeout {
        message: String,
    },
    Other(AnyhowError),
}

impl PoolLookupError {
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable {
            message: message.into(),
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::Timeout {
            message: message.into(),
        }
    }

    pub fn other(error: impl Into<AnyhowError>) -> Self {
        Self::Other(error.into())
    }

    pub fn kind(&self) -> PoolLookupErrorKind {
        match self {
            Self::Unavailable { .. } => PoolLookupErrorKind::Unavailable,
            Self::Timeout { .. } => PoolLookupErrorKind::Timeout,
            Self::Other(_) => PoolLookupErrorKind::Other,
        }
    }
}

impl fmt::Display for PoolLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { message } => write!(f, "pool lookup unavailable: {message}"),
            Self::Timeout { message } => write!(f, "pool lookup timed out: {message}"),
            Self::Other(error) => write!(f, "pool lookup failed: {error}"),
        }
    }
}

impl Error for PoolLookupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Other(error) => Some(error.root_cause()),
            Self::Unavailable { .. } | Self::Timeout { .. } => None,
        }
    }
}

impl From<AnyhowError> for PoolLookupError {
    fn from(error: AnyhowError) -> Self {
        Self::Other(error)
    }
}

#[non_exhaustive]
#[derive(Debug)]
pub enum HostError {
    #[non_exhaustive]
    InvalidDispatchRequest {
        protocol: &'static str,
        request_kind: &'static str,
    },
    PoolLookup(PoolLookupError),
    Targeting(String),
    CoreInvalidRequest(String),
    CorePoolNotFound(String),
    CoreEndpointNotFound(String),
    CoreBuild(String),
    #[non_exhaustive]
    CoreAllEndpointsSaturated {
        pool_id: Option<String>,
    },
    #[non_exhaustive]
    CoreNoAvailableEndpoint {
        pool_id: Option<String>,
    },
    #[non_exhaustive]
    CoreUpstreamHttp {
        status: u16,
        body: Option<String>,
        endpoint_id: String,
    },
    #[non_exhaustive]
    CoreTransport {
        message: String,
        endpoint_id: Option<String>,
    },
    #[non_exhaustive]
    CoreStreamAborted {
        message: String,
        endpoint_id: String,
    },
    CoreNotImplemented(&'static str),
    #[non_exhaustive]
    CoreAllAttemptsFailed {
        attempts: Vec<AttemptReport>,
        last_error: Box<HostError>,
    },
}

impl HostError {
    pub fn invalid_dispatch_request(protocol: &'static str, request_kind: &'static str) -> Self {
        Self::InvalidDispatchRequest {
            protocol,
            request_kind,
        }
    }

    pub fn pool_lookup(error: impl Into<PoolLookupError>) -> Self {
        Self::PoolLookup(error.into())
    }

    pub fn targeting(error: impl fmt::Display) -> Self {
        Self::Targeting(error.to_string())
    }

    pub fn core(error: GatewayError) -> Self {
        Self::from(error)
    }

    pub fn upstream_status_code(&self) -> Option<u16> {
        match self {
            Self::CoreUpstreamHttp { status, .. } => Some(*status),
            Self::CoreAllAttemptsFailed { last_error, .. } => last_error.upstream_status_code(),
            _ => None,
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDispatchRequest {
                protocol,
                request_kind,
            } => write!(
                f,
                "invalid dispatch request: protocol {protocol} does not accept {request_kind}"
            ),
            Self::PoolLookup(error) => write!(f, "pool lookup failed: {error}"),
            Self::Targeting(message) => write!(f, "target resolution failed: {message}"),
            Self::CoreInvalidRequest(message) => write!(f, "core invalid request: {message}"),
            Self::CorePoolNotFound(pool_id) => write!(f, "core pool not found: {pool_id}"),
            Self::CoreEndpointNotFound(endpoint_id) => {
                write!(f, "core endpoint not found: {endpoint_id}")
            }
            Self::CoreBuild(message) => write!(f, "core build failed: {message}"),
            Self::CoreAllEndpointsSaturated { pool_id } => {
                write!(f, "all endpoints saturated for pool: {pool_id:?}")
            }
            Self::CoreNoAvailableEndpoint { pool_id } => {
                write!(f, "no available endpoint for pool: {pool_id:?}")
            }
            Self::CoreUpstreamHttp {
                status,
                endpoint_id,
                ..
            } => write!(
                f,
                "upstream http error {status} from endpoint {endpoint_id}"
            ),
            Self::CoreTransport {
                message,
                endpoint_id,
            } => write!(f, "transport error on {endpoint_id:?}: {message}"),
            Self::CoreStreamAborted {
                message,
                endpoint_id,
            } => write!(f, "stream aborted on {endpoint_id}: {message}"),
            Self::CoreNotImplemented(feature) => {
                write!(f, "core feature not implemented: {feature}")
            }
            Self::CoreAllAttemptsFailed { last_error, .. } => {
                write!(f, "core execution exhausted all attempts: {last_error}")
            }
        }
    }
}

impl Error for HostError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PoolLookup(error) => Some(error),
            Self::CoreAllAttemptsFailed { last_error, .. } => Some(last_error.as_ref()),
            Self::InvalidDispatchRequest { .. }
            | Self::Targeting(_)
            | Self::CoreInvalidRequest(_)
            | Self::CorePoolNotFound(_)
            | Self::CoreEndpointNotFound(_)
            | Self::CoreBuild(_)
            | Self::CoreAllEndpointsSaturated { .. }
            | Self::CoreNoAvailableEndpoint { .. }
            | Self::CoreUpstreamHttp { .. }
            | Self::CoreTransport { .. }
            | Self::CoreStreamAborted { .. }
            | Self::CoreNotImplemented(_) => None,
        }
    }
}

impl From<GatewayError> for HostError {
    fn from(error: GatewayError) -> Self {
        match error {
            GatewayError::PoolNotFound(pool_id) => Self::CorePoolNotFound(pool_id),
            GatewayError::EndpointNotFound(endpoint_id) => Self::CoreEndpointNotFound(endpoint_id),
            GatewayError::InvalidRequest(message) => Self::CoreInvalidRequest(message),
            GatewayError::BuildError(message) => Self::CoreBuild(message),
            GatewayError::AllEndpointsSaturated { pool_id } => {
                Self::CoreAllEndpointsSaturated { pool_id }
            }
            GatewayError::NoAvailableEndpoint { pool_id } => {
                Self::CoreNoAvailableEndpoint { pool_id }
            }
            GatewayError::AllAttemptsFailed {
                attempts,
                last_error,
            } => Self::CoreAllAttemptsFailed {
                attempts,
                last_error: Box::new(Self::from(*last_error)),
            },
            GatewayError::UpstreamHttp {
                status,
                body,
                endpoint_id,
            } => Self::CoreUpstreamHttp {
                status,
                body,
                endpoint_id,
            },
            GatewayError::Transport {
                message,
                endpoint_id,
            } => Self::CoreTransport {
                message,
                endpoint_id,
            },
            GatewayError::StreamAborted {
                message,
                endpoint_id,
            } => Self::CoreStreamAborted {
                message,
                endpoint_id,
            },
            GatewayError::NotImplemented(feature) => Self::CoreNotImplemented(feature),
        }
    }
}
