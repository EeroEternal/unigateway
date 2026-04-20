# `unigateway` (deprecated)

**Do not depend on this crate for new projects.**

Use [`unigateway-sdk`](https://crates.io/crates/unigateway-sdk) instead. It is the supported facade over `unigateway-core`, `unigateway-protocol`, and `unigateway-host`.

This `unigateway` crate is a **compatibility shim** that re-exports `unigateway-sdk`. The standalone `ug` binary and in-tree HTTP product shell have been removed from the repository; build your own gateway binary if you need HTTP/CLI.
