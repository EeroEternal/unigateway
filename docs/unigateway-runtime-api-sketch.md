# Draft: `unigateway-runtime` API Sketch

Status: Draft

Date: 2026-04-06

Related documents:

- [docs/unigateway-gateway-runtime-design.md](docs/unigateway-gateway-runtime-design.md)
- [docs/unigateway-core-api-draft.md](docs/unigateway-core-api-draft.md)

## 1. Purpose

This document defines the first explicit API shape that the future `unigateway-runtime` crate should converge toward.

The goal is not to freeze exact Rust signatures yet. The goal is to make the intended facade clear enough that extraction becomes mostly file movement and visibility changes, not another round of architecture discovery.

## 2. Design Rule

The future crate should expose grouped facade modules that describe responsibilities, not internal file layout.

The old in-repo `crate::runtime` staging boundary served this extraction, but product code now imports `unigateway_runtime` directly.

The intended top-level grouped surface is:

- `unigateway_runtime::host`
- `unigateway_runtime::core`
- `unigateway_runtime::flow`
- `unigateway_runtime::status`

Current extraction status:

- `unigateway_runtime::host` exists as a real workspace crate surface
- `unigateway_runtime::core` exists as a real workspace crate surface
- `unigateway_runtime::flow` exists as a real workspace crate surface
- `unigateway_runtime::status` exists as a real workspace crate surface

The product crate still keeps a local adapter compilation unit at `src/runtime_host_adapter.rs`.
Product call sites should import `unigateway_runtime` directly; `src/runtime_host_adapter.rs` exists mainly to compile the `AppState` host trait implementations.

The product crate also keeps two explicitly product-owned edge adapters:

- `src/protocol.rs` for wire JSON normalization into `unigateway_core::Proxy*Request`
- `src/gateway/legacy_runtime.rs` for `llm-connector`-backed compatibility fallback

Those remain outside `unigateway-runtime` so the runtime crate can stay free of `llm-connector` and other product-shell transport details.

Implementation files behind those grouped modules should remain private details whenever practical.

## 3. `host`

Purpose:

- define host capability contracts
- define the runtime context object used by execution paths
- keep inversion of control explicit

Expected public concepts:

- `ResolvedProvider`
- `RuntimeContext`
- `RuntimeConfig`
- `RuntimeFuture`
- `RuntimeConfigHost`
- `RuntimeEngineHost`
- `RuntimePoolHost`
- `RuntimeRoutingHost`

The current runtime boundary assumes service-backed pools are synchronized into `unigateway-core` ahead of request execution.
`RuntimePoolHost` therefore acts as a lookup for already-synced pools, not as a per-request builder.

First-pass host signatures should converge toward:

```rust
pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct RuntimeConfig<'a> {
	pub openai_base_url: &'a str,
	pub openai_api_key: &'a str,
	pub openai_model: &'a str,
	pub anthropic_base_url: &'a str,
	pub anthropic_api_key: &'a str,
	pub anthropic_model: &'a str,
}

pub trait RuntimeConfigHost: Send + Sync {
	fn runtime_config(&self) -> RuntimeConfig<'_>;
}

pub trait RuntimeEngineHost: Send + Sync {
	fn core_engine(&self) -> &UniGatewayEngine;
}

pub trait RuntimePoolHost: Send + Sync {
	fn pool_for_service<'a>(
		&'a self,
		service_id: &'a str,
	) -> RuntimeFuture<'a, anyhow::Result<Option<ProviderPool>>>;
}

pub trait RuntimeRoutingHost: Send + Sync {
	fn resolve_providers<'a>(
		&'a self,
		service_id: &'a str,
		protocol: &'a str,
		hint: Option<&'a str>,
	) -> RuntimeFuture<'a, anyhow::Result<Vec<ResolvedProvider>>>;
}

impl<'a> RuntimeContext<'a> {
	pub fn from_parts(
		config_host: &'a dyn RuntimeConfigHost,
		engine_host: &'a dyn RuntimeEngineHost,
		pool_host: &'a dyn RuntimePoolHost,
		routing_host: &'a dyn RuntimeRoutingHost,
	) -> Self;
}
```

Likely not public in v1:

- product-specific `AppState` implementations
- any config-file-specific or admin-specific host adapters
- a composite `RuntimeHost` trait alias used only as a shortcut constructor input

Rationale:

- embedders need to satisfy host contracts
- embedders should not depend on UniGateway product state types
- request handlers should not rebuild config-derived pools on the hot path
- requiring explicit `from_parts(...)` keeps the capability boundary visible and avoids leaking an unnecessary convenience trait into the stable API

## 4. `core`

Purpose:

- expose core-first runtime execution entry points
- bridge normalized `unigateway_core::Proxy*Request` values into `unigateway-core`

Expected public concepts or entry points:

- OpenAI chat via core
- OpenAI responses via core
- OpenAI embeddings via core
- Anthropic chat via core
- env-key variants where runtime compatibility intentionally supports them

Current in-repo examples:

- `try_openai_chat_via_core`
- `try_openai_responses_via_core`
- `try_openai_embeddings_via_core`
- `try_anthropic_chat_via_core`
- `responses_payload_is_core_compatible`
- `embeddings_payload_is_core_compatible`

Rationale:

- upper protocol/runtime layers need one clear place to attempt core-first execution
- these entry points are runtime semantics, not product semantics

## 5. `flow`

Purpose:

- centralize runtime-owned orchestration helpers that are shared by product adapters
- keep env-key fallback policy with runtime-owned error shaping in one place

Expected public concepts:

- runtime flow resolution helpers for authenticated and env-key execution
- a shared runtime response result type used by product wrappers
- env runtime config helpers
- missing upstream key response helper

Current in-repo examples:

- `resolve_authenticated_runtime_flow`
- `resolve_env_runtime_flow`
- `prepare_openai_env_config`
- `prepare_anthropic_env_config`
- `missing_upstream_api_key_response`

Rationale:

- this logic is runtime semantics rather than product wiring
- product code may still own endpoint-specific orchestration, but it should rely on shared runtime flow policy rather than reimplementing env/error handling ad hoc

## 6. `status`

Purpose:

- centralize runtime-owned status mapping policy for runtime/core/legacy failures

Expected public concepts:

- `status_for_core_error`
- `status_for_legacy_error`

Rationale:

- product adapters should not re-derive status mapping ad hoc
- keeping this grouped makes error policy auditable and reusable

## 7. Not in the First Public Surface

The following should remain internal or product-owned in the first extraction:

- `gateway/support/*` request and execution orchestration helpers
- Axum route functions in `src/gateway.rs`
- wire JSON normalization in `src/protocol.rs`
- `llm-connector` fallback transport in `src/gateway/legacy_runtime.rs`
- product middleware types such as `GatewayAuth`
- admin/config persistence logic
- CLI and MCP flows

These may eventually feed into a public HTTP adapter layer, but they should not be treated as the initial stable crate API.

## 8. Extraction Heuristic

When evaluating whether a symbol belongs in the future `unigateway-runtime` public surface, use this test:

1. would an external embedder need this symbol directly?
2. does the symbol describe runtime semantics rather than product wiring?
3. can the symbol remain stable even if UniGateway product code changes?

If the answer is not clearly yes, the symbol should remain internal for now.

## 9. Current Mapping Summary

The current extracted mapping should be treated as the working draft:

- product code imports `unigateway_runtime::host`
- product code imports `unigateway_runtime::core`
- product code imports `unigateway_runtime::flow`
- product code imports `unigateway_runtime::status`
- product code imports local `crate::protocol`
- product code imports local `crate::gateway::legacy_runtime`

This does not freeze exact signatures yet, but it does freeze the intended semantic grouping.