//! # Deprecated crate name
//!
//! **`unigateway` is not the supported embedding entry point.** Use
//! **[`unigateway-sdk`](https://crates.io/crates/unigateway-sdk)** for new code and
//! cross-crate version alignment.
//!
//! This crate exists so crates.io can carry a clear deprecation message and a thin
//! re-export for any legacy `unigateway = "…"` dependency lines. The historical `ug`
//! CLI binary is **not** shipped from this repository.

pub use unigateway_sdk::*;
