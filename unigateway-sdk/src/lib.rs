#![warn(missing_docs)]
//! Thin facade crate for UniGateway embedders.
//!
//! This crate intentionally does very little:
//! it re-exports the underlying crates under stable namespaces and keeps
//! feature selection/version alignment in one place.
//!
//! For self-managed HTTP (no in-process engine), enable
//! `default-features = false, features = ["conversion"]`.

#[cfg(any(feature = "core", feature = "conversion"))]
pub use unigateway_core as core;

#[cfg(any(feature = "protocol", feature = "conversion"))]
pub use unigateway_protocol as protocol;

#[cfg(feature = "host")]
pub use unigateway_host as host;

#[cfg(feature = "session")]
pub use unigateway_session as session;

#[cfg(feature = "config")]
pub use unigateway_config as config;
