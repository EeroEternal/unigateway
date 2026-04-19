pub mod core;
pub mod env;
pub mod error;
pub mod host;
pub mod status;

pub use crate::core::{
    HostDispatchOutcome, HostDispatchTarget, HostProtocol, HostRequest,
    anthropic_requested_model_alias, dispatch_request,
};
pub use crate::env::{EnvPoolHost, EnvProvider, build_env_pool};
pub use crate::error::{
    HostError, HostResult, PoolLookupError, PoolLookupErrorKind, PoolLookupResult,
};
pub use crate::host::{HostContext, HostFuture, PoolHost, PoolLookupOutcome};

#[cfg(feature = "testing")]
pub mod testing;
