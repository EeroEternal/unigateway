# Changelog

All notable changes to this project are documented in this file.

## [1.2.0]

We are thrilled to announce **UniGateway v1.2.0**, marking our most stable, secure, and developer-friendly release yet. This release jumps directly from the v0.x / v1.0 iterations, consolidating all critical architectural polishing and cleanup!

### Highlights

#### 1. Context-Aware Diagnostics & Fail-Fast Engineering
* **Contextual Errors**: `GatewayError::NoAvailableEndpoint` now precisely injects `pool_id` under the hood. Debugging routing failures is now instantaneous.
* **Fail-Fast Engine Builder**: Building a gateway without an explicit Driver Registry now results in an immediate, safe `BuildError` instead of a ticking runtime failure.

#### 2. Bulletproof Reliability
* **Graceful Shutdown**: The gateway now properly handles `SIGTERM` and `Ctrl+C`, pausing traffic ingestion but letting existing inference streams finish gracefully before terminating. State mutations (e.g., quota consumption) are securely synced up on the exit.

#### 3. Deep Telemetry & PII Scrubbing
* **Gateway Hooks**: Refactored the core events (`AttemptStartedEvent`) to strictly isolate AI inputs (prompts, API keys) from the telemetry buses.
* **Zero-Leak Logging**: By default, the unified console logger now only emits metadata (Endpoints, Pool IDs, Latency, Upstream Codes) without ever exposing PII.

#### 4. Code & DX Improvements
* **100% Rustdoc Coverage**: Core crates (`engine`, `hooks`, `drivers`, `error`) are now thoroughly documented under the strict `#![warn(missing_docs)]` lint, providing a world-class embeddable gateway DX.
* **Architecture Docs Decruft**: Removed legacy drafts, check-sheets, and old iteration plans from the `docs/` folder, maintaining a much leaner and cleaner OSS footprint.

#### 5. Dependency & Tooling Update
* **Rust 1.92 Ready**: Fully cached and formatted across the CI.
* **Skills Updated**: The Universal CLI skill (`SKILL.md` and `openapi.yaml`) definition is bumped to v1.2.0 natively.

**Upgrade Note:** As part of this release, the engine builder has been tightened. If embedding `unigateway-core` directly, make sure to handle the `Result` in `UniGatewayEngine::builder().build()`.
