#![warn(missing_docs)]
//! Thin facade crate for UniGateway embedders.
//!
//! This crate intentionally does very little:
//! it re-exports the underlying crates under stable namespaces and keeps
//! feature selection/version alignment in one place.

#[cfg(feature = "core")]
pub use unigateway_core as core;

#[cfg(feature = "protocol")]
pub use unigateway_protocol as protocol;

#[cfg(feature = "host")]
pub use unigateway_host as host;
