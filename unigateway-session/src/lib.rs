//! Reference session prefix primitives for agent-style embedders.
//!
//! Enable the `http` feature for optional Axum publish/delete routes.

pub mod lifecycle;
pub mod lifetime;
pub mod middleware;
pub mod store;

pub use lifecycle::{SessionLifecycleEvent, SessionLifecycleHook, SessionSizeRejectKind};
pub use lifetime::{SessionLifetime, is_session_expired};
pub use middleware::{
    DeltaAssemblyMiddleware, SessionDelivery, SessionGatewayContext, SessionKeyResolver,
    SessionMiddlewareConfig, TailPositionPolicy,
};
pub use store::{
    DEFAULT_NAMESPACE, Fingerprint, FingerprintPolicy, MemorySessionStore, PublishResult,
    SessionError, SessionKey, SessionPrefix, SessionSizeLimits, SessionStore, SessionStoreConfig,
    SessionStoreError, fingerprints_match, message_json_bytes,
};

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "http")]
pub use http::{SessionHttpConfig, session_router};

/// Default gateway field key for session delivery hints (`gateway_fields`).
pub const SESSION_GATEWAY_FIELD: &str = "_session_context";
