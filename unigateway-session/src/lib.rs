//! Reference session prefix primitives for agent-style embedders.
//!
//! Enable the `http` feature for optional Axum publish/delete routes.

pub mod middleware;
pub mod store;

pub use middleware::{DeltaAssemblyMiddleware, SessionDelivery, SessionGatewayContext};
pub use store::{MemorySessionStore, SessionPrefix, SessionStoreError};

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "http")]
pub use http::{SessionHttpConfig, session_router};

/// Default gateway field key for session delivery hints (`gateway_fields`).
pub const SESSION_GATEWAY_FIELD: &str = "_session_context";
