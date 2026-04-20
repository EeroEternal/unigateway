# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [1.6.0]

UniGateway v1.6.0 is a **product-shape release**: the repository is now a **library workspace** only. The supported public dependency for new projects is **`unigateway-sdk`**.

### Breaking Changes (product)

* **Removed the standalone `ug` binary and the root product crate** (`src/` HTTP server, admin/MCP glue, middleware). **`unigateway-cli` has been removed** from this repository. Embed and ship your own process/HTTP surface using **`unigateway-sdk`** (and optional direct `unigateway-*` crates). Release CI no longer publishes `ug` tarball artifacts or updates the Homebrew formula for `ug`.
* **Removed the `skills/` directory** (`SKILL.md`, `openapi.yaml`) that documented the old CLI-oriented agent skill; gateway HTTP contracts should be maintained by the host application that embeds this workspace.

### Crates.io

* **`unigateway` crate**: republished as a **deprecated compatibility shim** that re-exports `unigateway-sdk`. Its `description` and README state that new code should depend on `unigateway-sdk` instead; the `ug` binary is not shipped from this crate.

### Validation

* Workspace `fmt`, `clippy -D warnings`, and `test` pass on the 1.6.0 line.

## [1.5.2]

UniGateway v1.5.2 is a patch release focused on Anthropic protocol fidelity, especially for tools, thinking, and cross-provider compatibility.

### Fixes

* **Preserved Anthropic request semantics through the core chat model**: chat requests now keep `system`, raw `messages`, `tools`, `tool_choice`, `top_k`, and `stop_sequences` intact so Anthropic-native upstreams no longer lose protocol-specific fields.
* **Completed Anthropic-to-OpenAI tool translation for OpenAI-compatible upstreams**: Anthropic `tool_use`, `tool_result`, `thinking`, and tool schemas now translate into OpenAI-compatible message, tool-call, and function-tool payloads when the selected upstream is not Anthropic-native.
* **Completed OpenAI-to-Anthropic response rendering for tools and thinking**: Anthropic-compatible completed bodies and SSE streams now emit `tool_use`, `input_json_delta`, `thinking_delta`, `signature_delta`, Anthropic-style message IDs, and cache token usage fields for clients expecting `/v1/messages` behavior.

### Validation

* **Anthropic compatibility coverage now includes request parsing, native-driver passthrough, cross-protocol tool conversion, and fine-grained streaming regressions**: workspace `fmt`, `test`, and `clippy` all pass on the 1.5.2 release line.

## [1.5.1]

UniGateway v1.5.1 is a patch release focused on hardening Anthropic-compatible gateway auth expectations for downstream tools like Cherry Studio.

### Fixes

* **Locked in `x-api-key` auth extraction behavior with regression tests**: the gateway now has explicit coverage ensuring Anthropic-style `x-api-key` headers are preferred on `/v1/messages`, Bearer auth still works as a compatibility fallback, and OpenAI-compatible entry points continue accepting `x-api-key` for clients that send it.

### Validation

* **Header extraction regressions are now covered in unit tests**: this release adds targeted tests around gateway-key parsing so future refactors do not silently regress Cherry Studio and other Anthropic-style clients.

## [1.5.0]

UniGateway v1.5.0 is the follow-up embedder-contract release. It isolates the second round of public host-surface tightening from the already-published v1.4.0 line instead of silently mutating that release after publish.

### Breaking Changes

* **`PoolHost::pool_for_service` and `EnvPoolHost::env_pool` now return `PoolLookupResult<PoolLookupOutcome>`**: the error side is now `PoolLookupError` instead of a generic `anyhow::Error`. Migration: update the trait signatures and replace `Err(anyhow!(...))` with `Err(PoolLookupError::other(...))` or a more specific `PoolLookupError` variant.
* **Public host enums are now non-exhaustive**: `HostError`, `HostRequest`, `HostDispatchTarget`, and `HostDispatchOutcome` are now `#[non_exhaustive]`, and the public struct-style `HostError` variants are also non-exhaustive. External code must keep wildcard match arms and can no longer construct those struct-style variants directly with brace syntax.
* **`unigateway_host::flow` has been removed from the public API**: product-shell-specific HTTP response shaping now lives only in the root crate. Embedders should stay on structured `HostDispatchOutcome` / `HostError` values and perform framework adaptation in their own application layer.

### Highlights

#### 1. Host Contract Tightening
* **Typed pool lookup errors**: embedders can now distinguish unavailable, timeout, and fallback lookup failures through `PoolLookupErrorKind` / `PoolLookupError` instead of depending on stringified `anyhow` errors.
* **Response shaping stays outside the host crate**: `unigateway-host` now stops at structured dispatch outcomes and typed errors; the root product shell performs the final HTTP response mapping.

#### 2. Docs And Surface Cleanup
* **`Endpoint` rustdoc clarified**: hint-matching guidance now documents the `Endpoint` struct itself instead of accidentally attaching to the `endpoint_id` field.
* **Embedder docs moved to the 1.5 line**: README, embed guide, SDK README, and dev notes now all describe the post-1.4 public contract as a 1.5.0 release.

#### 3. Tooling And Validation
* **SDK feature CI wording aligned with reality**: release docs now explicitly mention that CI exercises `core`, `protocol`, `host`, `embed`, and `testing` feature combinations.
* **Gateway stats now record real response statuses**: the root gateway response path records the actual HTTP status code instead of collapsing all host-side failures to a synthetic 500 in metrics.

**Upgrade Note:** If you implemented `PoolHost` or `EnvPoolHost` against v1.4.0, treat v1.5.0 as a real source update: migrate the trait error type, add wildcard arms when matching non-exhaustive host enums, and remove any dependency on `unigateway_host::flow`.

## [1.4.0]

UniGateway v1.4.0 is the embedder-contract release. It keeps the multi-crate direction from v1.3.0, but treats the latest host-facing API tightening as a semver-significant upgrade instead of shipping it on the v1.3.x line.

### Breaking Changes

* **`PoolHost` / `EnvPoolHost` return signatures changed**: `Result<Option<ProviderPool>>` has been replaced by `Result<PoolLookupOutcome>`.
* **Embedder implementations must update trait impls**: downstream hosts that implement `PoolHost::pool_for_service` or `EnvPoolHost::env_pool` now need to return `PoolLookupOutcome::Found(...)` / `PoolLookupOutcome::NotFound`.
* **`PoolLookupOutcome` is non-exhaustive**: embedders matching on lookup results should keep a fallback arm so future host versions can add richer states without another immediate rewrite.

### Highlights

#### 1. Host Contract Tightening
* **Typed host errors**: host dispatch and flow code now return `HostError`, separating dispatch mismatch, pool lookup failure, targeting failure, and core execution failure.
* **Explicit pool lookup outcome**: service/env pool resolution no longer overloads `None`; dispatch paths now treat missing pools as an explicit host-side outcome.
* **Dispatch semantics clearer**: root gateway execution keeps env fallback target resolution explicit instead of threading `Option` through internal control flow.

#### 2. SDK Facade Polish
* **Canonical full-stack feature renamed in practice**: `host` is now the canonical named full-stack feature, while `embed` remains as a 1.x compatibility alias.
* **SDK docs tightened**: README and embedder guides now recommend `unigateway-sdk = "1.4"` as the primary entry point and describe the compatibility policy in release-line terms.

#### 3. Tooling And Validation
* **SDK feature CI broadened**: CI keeps checking the `core`, `protocol`, `host`, `embed`, and `testing` feature combinations.
* **Workspace validation rerun**: format, tests, clippy, and SDK feature-set compilation all pass on the 1.4.0 line.

**Upgrade Note:** If you implement `PoolHost` or `EnvPoolHost` directly, migrate `Ok(Some(pool))` to `Ok(PoolLookupOutcome::found(pool))` and `Ok(None)` to `Ok(PoolLookupOutcome::not_found())` before upgrading.

## [1.3.0]

UniGateway v1.3.0 is the refactor release that turns the repository into a cleaner multi-crate workspace and significantly narrows the root product shell.

### Highlights

#### 1. Workspace Split And Naming Cleanup
* **Dedicated crates**: config, protocol, host, and CLI responsibilities now live in `unigateway-config`, `unigateway-protocol`, `unigateway-host`, and `unigateway-cli` instead of being folded into the root crate.
* **Runtime renamed to host**: the old `unigateway-runtime` surface has been physically renamed and narrowed to a host bridge with clearer contracts.

#### 2. Narrow Runtime State Boundaries
* **Three HTTP surfaces**: system, gateway, and admin routes now mount with dedicated state types instead of sharing a wide `AppState` at request time.
* **Gateway request isolation**: middleware, host adapter, and gateway request support flows now run on `GatewayRequestState`.
* **Admin isolation**: admin CRUD, metrics, and MCP management all live under `src/admin/` and use `AdminState`.

#### 3. Thinner Root Product Shell
* **System router extracted**: `/health`, `/metrics`, and `/v1/models` now run through `SystemState` and a dedicated system router.
* **Config access tightened**: root code no longer reaches directly into `GatewayState` internals for runtime quotas and queue state.

#### 4. GatewayState Split
* **Config store + runtime limiter**: `GatewayState` now composes a durable config store and a separate in-memory runtime limiter instead of carrying both concerns as one monolith.
* **Core sync remains explicit**: config-to-core pool projection continues to be driven through explicit sync methods rather than ad hoc state reads.

#### 5. Docs And Contributor Model Updated
* **Refactor baseline refreshed**: contributor docs now describe the current workspace split, the narrowed runtime states, and the remaining architectural debt.
* **Skills bumped**: MCP/OpenAPI skill metadata now targets v1.3.0.

#### 6. New Embedder Facade Crate
* **`unigateway-sdk` added**: embedders now have a single dependency entry point that re-exports `unigateway-core`, `unigateway-protocol`, and `unigateway-host` under namespaced modules.
* **Thin facade by design**: the SDK only centralizes feature selection and version alignment; it does not introduce a second state model or execution abstraction.
* **Version policy documented**: SDK consumers are expected to keep `unigateway-sdk`, `unigateway-core`, `unigateway-protocol`, and `unigateway-host` on the same minor line.

**Upgrade Note:** If you embed UniGateway crates directly, pay attention to the crate rename from `unigateway-runtime` to `unigateway-host`, the new protocol crate boundary, and the narrower host/request state contracts.

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
