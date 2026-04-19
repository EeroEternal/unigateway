# UniGateway Project Memory

This document is optimized for contributors and AI agents that need a fast but accurate mental model of the current UniGateway codebase.

## One-Screen Summary

UniGateway is a local-first LLM gateway with a CLI-first product shell, a reusable config state crate, a reusable host bridge, a reusable core execution engine, and a published embedder facade crate.

The repository currently has five main layers:

1. Product shell in `src/`
  - HTTP server, admin API surface, gateway authentication, telemetry, and product-shell workflows.
  - Root `src/main.rs` and `src/admin/mcp.rs` call `unigateway-cli` directly for CLI process-management concerns.
2. Config state in `unigateway-config/`
  - TOML-backed `GatewayState`, admin / CLI mutations, routing helpers, and config → core pool projection.
3. Host bridge in `unigateway-host/`
  - Converts product-level state into a stable host contract, exposes a unified dispatch API, returns typed `HostError` values for embedder-facing failures, and translates core results into protocol-owned neutral HTTP response payloads.
4. Core execution engine in `unigateway-core/`
  - Manages provider pools, endpoint selection, retry / fallback policy, driver execution, streaming completion, and request reports.
5. Embedder facade in `unigateway-sdk/`
  - Re-exports `unigateway-core`, `unigateway-protocol`, and `unigateway-host` under a single namespaced dependency without adding a second abstraction layer.

The most important architectural shift is this:

- Old mental model: gateway handlers directly parse payloads, route to providers, and call upstreams.
- Current mental model: product shell prepares requests, host-layer code resolves execution targets, and `unigateway-core` performs the actual provider execution.
- Embedder entry model: external applications should usually start from `unigateway-sdk`, then reach through to `core`, `protocol`, and `host` namespaces as needed.

## Product Identity

UniGateway is intended to be the stable local entry point between AI tools and multiple upstream model providers.

Primary goals:

- One local base URL for multiple tools.
- One user-facing abstraction for switching between upstream providers: `mode`.
- Reliable failover / fallback without every tool needing custom logic.
- Easy local setup through CLI and config file.
- Good operator visibility through route explainers, diagnostics, metrics, and request reports.

## Core Terminology

Several terms refer to similar ideas at different layers. This is the most important vocabulary map in the repo.

### User-facing terms

- `mode`
  - The user-facing name for a routing intent such as `default`, `fast`, or `strong`.
- `service`
  - The persisted config-level object that backs a mode.
  - In most runtime paths, `mode` and `service` effectively refer to the same thing.
- `provider`
  - A configured upstream provider entry from the TOML config.
- `binding`
  - Connects a service to one provider with a priority.

### Core engine terms

- `pool`
  - The execution object stored inside `UniGatewayEngine`.
  - A service is projected into a pool during core sync.
- `endpoint`
  - A single executable upstream target inside a pool.
- `ExecutionTarget`
  - The target shape passed to the core engine for one request.
  - Usually either `Pool { pool_id }` or `Plan { candidates, ... }`.
- `driver`
  - Provider protocol implementation such as `openai-compatible` or `anthropic`.

### Authentication terms

- `gateway api key`
  - UniGateway-managed key stored in config and used for service-based routing.
- `upstream api key`
  - Real provider credential, either stored in provider config or supplied via environment fallback.

## Repository Layout

### Root crate: product shell

Main responsibilities:

- CLI entry and command dispatch
- HTTP route registration
- admin API
- gateway authentication and request limits
- state assembly and core engine lifecycle

Key files:

- `unigateway-cli/src/lib.rs`
  - `Cli`, `Commands`, subcommand action enums, shared `GuideCommand`, plus the public CLI execution surface re-exported from `diagnostics`, `guide`, `modes`, `process`, `render/*`, and `setup`.
- `unigateway-cli/src/setup.rs`
  - Interactive setup / quickstart flow and provider prompt orchestration.
- `unigateway-cli/src/tests.rs`
  - CLI regression tests for guide flow, route explanations, integration rendering, and parsing helpers.
- `src/main.rs`
  - CLI entry point and top-level dispatch.
- `src/server.rs`
  - HTTP server startup, route registration, app state wiring, and background config-to-core sync trigger.
- `src/types.rs`
  - `AppConfig` and `AppState`.
- `src/middleware.rs`
  - Gateway key auth, quota, QPS, concurrency limits.
- `src/gateway.rs`
  - Thin HTTP handlers only.
- `src/gateway/support/request_flow.rs`
  - Build `HostContext`, extract token / provider hint, authenticate, and parse typed request.
- `src/gateway/support/execution_flow.rs`
  - Main bridge from prepared request into runtime/core execution.

### `unigateway-config/`: config state crate

Main responsibilities:

- Own TOML-backed gateway config state and persistence.
- Provide admin / CLI read-write helpers over `GatewayState`.
- Project config services/providers/bindings into `ProviderPool` values.
- Own config-scoped upstream resolution helpers.

Key files:

- `unigateway-config/src/lib.rs`
  - Crate root, exported types, constants, and `GatewayState`.
- `unigateway-config/src/runtime.rs`
  - Runtime-only API-key qps/concurrency limiter and queue metrics helpers.
- `unigateway-config/src/store.rs`
  - Load and persist config state.
- `unigateway-config/src/admin.rs`
  - Config mutation and admin-facing helpers.
- `unigateway-config/src/select.rs`
  - API-key lookup, stats, and read-only selection helpers.
- `unigateway-config/src/core_sync.rs`
  - Projection from config file model into core pools.
- `unigateway-config/src/routing.rs`
  - Upstream resolution and base URL normalization.

### `unigateway-host/`: host bridge

Main responsibilities:

- Define a stable host contract between product shell and reusable host logic.
- Delegate protocol parsing and neutral HTTP response shaping to `unigateway-protocol`.
- Materialize env-backed fallback pools through the host boundary.
- Expose unified dispatch over chat / responses / embeddings while keeping the root product shell thin.
- Return typed host errors while leaving HTTP response adaptation to the root product shell.

Key files:

- `unigateway-host/src/host.rs`
  - Defines `HostContext`, `PoolHost`, and explicit `PoolLookupOutcome` values for host-side pool resolution.
- `unigateway-host/src/error.rs`
  - Defines typed `HostError` / `HostResult` for dispatch mismatch, pool lookup, targeting, and core execution failures.
- `unigateway-host/src/env.rs`
  - Defines `EnvProvider`, `EnvPoolHost`, and env-backed fallback helpers for the product shell.
- `unigateway-protocol/src/lib.rs`
  - Re-exports protocol request parsers, response renderers, and neutral response types.
- `unigateway-protocol/src/requests.rs`
  - JSON payload to `Proxy*Request` translation.
- `unigateway-protocol/src/responses.rs`
  - `ProxySession` and completed response to the neutral protocol response type `ProtocolHttpResponse`, including SSE shaping.
- `unigateway-protocol/src/http_response.rs`
  - Neutral HTTP response body and streaming types shared by protocol rendering and the product shell.
- `unigateway-host/src/core/mod.rs`
  - Re-exports the host dispatch API.
- `unigateway-host/src/core/chat/mod.rs`
  - Target building and chat execution helpers used by dispatch.
- `unigateway-host/src/core/responses.rs`
  - OpenAI Responses API execution and stream compatibility fallback.
- `unigateway-host/src/core/embeddings.rs`
  - Embeddings execution wrapper.
- `unigateway-host/src/core/targeting.rs`
  - Build `ExecutionTarget` values and apply provider-hint matching.
- `unigateway-host/src/core/dispatch.rs`
  - Unified `dispatch_request` entry point, request/target enums, typed dispatch mismatch handling, and shared fallback helpers.
- `unigateway-host/src/status.rs`
  - Map typed `HostError` values to HTTP status codes for the product shell.

### `unigateway-sdk/`: embedder facade

Main responsibilities:

- Provide one dependency entry point for embedders.
- Re-export the underlying crates as `unigateway_sdk::core`, `unigateway_sdk::protocol`, and `unigateway_sdk::host`.
- Centralize feature selection and version-alignment guidance.

Key files:

- `unigateway-sdk/src/lib.rs`
  - Thin namespaced re-exports only.
- `unigateway-sdk/Cargo.toml`
  - Feature layout for `core`, `protocol`, `host`, and `embed`.
- `unigateway-sdk/README.md`
  - Version policy and facade positioning for embedders.

### `unigateway-core/`: reusable execution engine

Main responsibilities:

- Store provider pools in memory.
- Resolve execution targets into ordered endpoints.
- Execute chat / responses / embeddings via pluggable drivers.
- Apply retry conditions, backoff, fallback behavior, and request reporting.
- Normalize streaming completion and lifecycle hooks.

Key files:

- `unigateway-core/src/lib.rs`
  - Public API surface.
- `unigateway-core/src/engine/mod.rs`
  - Core engine state and builder.
- `unigateway-core/src/engine/execution.rs`
  - Chat / responses / embeddings attempt loops.
- `unigateway-core/src/engine/reporting.rs`
  - Retry decisions, reports, hooks, streaming completion finalization.
- `unigateway-core/src/routing.rs`
  - Build execution snapshots and endpoint ordering plans.
- `unigateway-core/src/protocol/mod.rs`
  - Built-in drivers and shared protocol utilities.
- `unigateway-core/src/protocol/openai/`
  - OpenAI-compatible driver, request builders, response parsers, streaming logic.
- `unigateway-core/src/protocol/anthropic.rs`
  - Anthropic driver and translation logic.

## The Three Main State Objects

### `GatewayState`

Defined in `unigateway-config/src/lib.rs` and re-exported through `src/config.rs`.

Responsibilities:

- Compose config-facing and runtime-facing sub-state for the gateway.
- Own the parsed TOML config file.
- Track runtime quota / rate state.
- Mark dirty state and persist changes.
- Trigger background sync into the core engine.
- Expose focused read/write helpers so product-shell code does not lock `inner` / `api_key_runtime` directly.

Current shape:

- `ConfigStore` owns the TOML-backed file state, dirty bit, and core-sync notifier.
- `RuntimeRateLimiter` owns per-key qps/concurrency tokens and queue bookkeeping.

Persistence model:

- Config file contents are durable.
- Runtime request counters and in-flight bookkeeping are memory-only.

### `AppState`

Defined in `src/types.rs`.

Responsibilities:

- Hold process-wide config defaults (`AppConfig`).
- Hold `GatewayState`.
- Hold the singleton `UniGatewayEngine`.
- Offer `sync_core_pools()` to refresh core execution state from config.

### `GatewayRequestState`

Defined in `src/types.rs`.

Responsibilities:

- Hold only the request-path dependencies needed by `/v1/*` gateway handlers.
- Back gateway auth/rate limiting, env-fallback provider config, and `HostContext` composition.
- Implement the host traits used by `src/gateway/support/*` without exposing the full `AppState` surface.

### `SystemState`

Defined in `src/types.rs`.

Responsibilities:

- Hold only the system-surface dependencies needed by `/health`, `/metrics`, and `/v1/models`.
- Expose request counters and default env-backed model names without carrying the full startup assembly state.

### `AdminState`

Defined in `src/admin/mod.rs`.

Responsibilities:

- Hold only the admin-facing dependencies: admin token, `GatewayState`, and core engine metrics access.
- Back `/api/admin/*` and `/v1/admin/queue_metrics` without exposing the full `AppState` surface.

### `HostContext`

Defined in `unigateway-host/src/host.rs`.

Responsibilities:

- Present a stable interface to host-layer logic.
- Decouple host crate logic from the product shell's concrete `AppState` type.

This is a major architectural boundary. Host-layer code should rely on traits and host capabilities rather than directly reaching into product-specific state.

## Startup Lifecycle

Current startup path:

1. `src/main.rs`
  - Parse CLI flags.
  - Build `AppConfig`.
2. `src/server.rs`
  - Load `GatewayState` from config file.
  - Construct `AppState`, which also constructs `UniGatewayEngine` with built-in HTTP drivers and telemetry hooks.
  - Derive `SystemState`, `GatewayRequestState`, and `AdminState` from `AppState` for narrower route surfaces.
  - Register a core-sync notifier.
  - Run `state.sync_core_pools()` once at startup.
3. Background sync loop in `src/server.rs`
  - Listens for config-change notifications.
  - Rebuilds and upserts config-managed pools.
4. Background persistence loop in `src/server.rs`
  - Periodically persists dirty config state.

Important consequence:

- The config file is not the direct execution source of truth for requests.
- Requests are served by the in-memory `UniGatewayEngine`, which is synchronized from config state.

## Config Projection: Service -> Pool

The single most important transformation in the product shell is in `src/config/core_sync.rs`.

Projection rules:

1. Every service becomes one `ProviderPool`.
2. Every binding contributes one candidate provider endpoint.
3. Each provider becomes one `Endpoint` with:
  - `provider_kind`
  - `driver_id`
  - resolved `base_url`
  - provider API key
  - parsed `ModelPolicy`
  - structured routing fields such as provider name, source endpoint id, and provider family, plus binding priority metadata.
4. Unsupported or invalid services are skipped or removed from core sync.
5. Config-managed pools are marked with metadata:
  - `managed_by = gateway-config`

Important implications:

- If a service has no enabled providers, the pool is not executable.
- If a provider has no API key or cannot resolve its upstream, core sync rejects it.
- If a config-managed service disappears, its corresponding engine pool is removed.

## Request Lifecycle

### System route path

1. Route entry in `src/server.rs`
  - `GET /health`
  - `GET /metrics`
  - `GET /v1/models`
2. Thin handler in `src/system.rs`
  - Runs with `SystemState`, not full `AppState`.

### OpenAI / Anthropic request path

1. Route entry in `src/server.rs`
  - `POST /v1/chat/completions`
  - `POST /v1/responses`
  - `POST /v1/embeddings`
  - `POST /v1/messages`
2. Thin handler in `src/gateway.rs`
  - Runs with `GatewayRequestState`, not full `AppState`.
3. Request preparation in `src/gateway/support/request_flow.rs`
  - Build `HostContext`
  - Extract token
  - Extract provider hint from headers / payload
  - Authenticate gateway key if present
  - Parse payload into typed request
4. Execution dispatch in `src/gateway/support/execution_flow.rs`
  - If gateway key matched: route by `service_id`
  - Otherwise: use environment fallback credentials
5. Host wrapper in `unigateway-host/src/core/*`
  - Build `ExecutionTarget`
  - Call `UniGatewayEngine`
  - Translate result into protocol response
6. Core engine in `unigateway-core`
  - Resolve pool / plan into ordered endpoints
  - Execute attempts via provider drivers
  - Apply retry / fallback rules
  - Build request report and streaming completion

### Authentication behavior

Implemented in `src/middleware.rs`.

Rules:

- If a gateway API key is present and valid, requests route through service-based execution.
- If no key matches, the request may fall back to environment-provided upstream credentials.
- Localhost compatibility shortcut exists:
  - If bind address is local and there is exactly one active gateway API key, an empty token can implicitly authenticate as that single key.

This implicit auth shortcut is easy to miss and important for AI tooling behavior.

## Routing Behavior

There are two routing layers in the repository.

### Product-level routing semantics

- Lives around services, providers, and bindings.
- Uses user-facing concepts such as mode selection and provider hints.

### Core execution routing semantics

- Lives around pools, endpoints, and execution plans.
- Uses `LoadBalancingStrategy` and `RetryPolicy`.

Current supported core strategies from config sync:

- `round_robin`
- `fallback`
- `random`

The runtime targeting layer can either:

- execute a whole pool, or
- construct a restricted `ExecutionPlan` with a filtered candidate endpoint subset.

## Driver Model

Built-in drivers are registered in `unigateway-core/src/protocol/mod.rs`.

Current built-ins:

- `openai-compatible`
- `anthropic`

Driver responsibilities:

- Build upstream HTTP requests.
- Parse non-streaming responses.
- Drive streaming frames into normalized chunk / event types.

Runtime does not talk to upstream HTTP directly. That belongs to core drivers.

## Streaming Model

The core engine normalizes streaming through:

- `ProxySession::Streaming`
- `StreamingResponse`
- typed chunks / events plus a completion handle

Runtime then adapts that into external protocol SSE.

Examples:

- OpenAI-compatible SSE passthrough or normalized event emission.
- Anthropic SSE compatibility stream generated from normalized chat chunks.

This means protocol translation is split:

- core driver normalizes provider stream into internal chunk/event shapes
- runtime adapts internal chunk/event shapes back into client-facing SSE formats

## Reporting And Observability

Core request reporting is built around `RequestReport` and `AttemptReport`.

Useful fields:

- selected endpoint
- selected provider kind
- per-attempt status and latency
- merged metadata from pool, endpoint, and request
- token usage
- total latency

Hooks can be attached to `UniGatewayEngine` via `GatewayHooks`. In the product shell, telemetry hooks are installed from `src/types.rs`.

## File Map By Concern

### Request ingress

- `src/server.rs`
- `src/gateway.rs`
- `src/gateway/support/request_flow.rs`
- `src/gateway/support/execution_flow.rs`

### Auth and limits

- `src/middleware.rs`

### Admin API

- `src/admin/mod.rs`
- `src/admin/mcp.rs`
- `src/admin/metrics.rs`
- `src/admin/service.rs`
- `src/admin/provider.rs`
- `src/admin/api_key.rs`

### Config and persistence

- `src/config.rs`
- `unigateway-config/src/store.rs`
- `unigateway-config/src/core_sync.rs`
- `unigateway-config/src/schema.rs`
- `unigateway-config/src/runtime.rs`

### Host bridge

- `src/host_adapter.rs`
- `unigateway-host/src/host.rs`
- `unigateway-host/src/core/*`
- `unigateway-host/src/status.rs`

### Core execution

- `unigateway-core/src/engine/*`
- `unigateway-core/src/routing.rs`
- `unigateway-core/src/protocol/*`
- `unigateway-core/src/transport.rs`

### Product CLI / UX

- `src/main.rs`
- `unigateway-cli/src/lib.rs`
- `unigateway-cli/src/render/`

## Common Extension Tasks

### Add a new HTTP endpoint

Likely touch:

1. `src/server.rs`
2. `src/gateway.rs`
3. `src/gateway/support/*`
4. `unigateway-host/src/core/*` if it needs a new host translation path
5. `unigateway-core/src/protocol/*` if it requires a new provider-level protocol call

### Add a new provider family

Likely touch:

1. `unigateway-core/src/protocol/`
2. `unigateway-core/src/protocol/mod.rs`
3. `src/config/core_sync.rs` for `provider_type -> driver_id / provider_kind` mapping
4. runtime translation only if external protocol compatibility needs special shaping

### Change service-to-provider selection behavior

Likely touch:

1. `src/config/core_sync.rs`
2. `unigateway-host/src/core/targeting.rs`
3. `unigateway-core/src/routing.rs`
4. `unigateway-core/src/engine/reporting.rs` if retry semantics change

## Known Non-Obvious Details

- The current `[docs/design/arch.md](../design/arch.md)` should describe the three-layer model, not the older direct gateway-to-upstream mental model.
- `GatewayRequestState` implements the host traits through the host adapter path, even though host-layer code only sees `HostContext`.
- Env fallback is still a first-class path for requests without gateway auth.
- Runtime and core crates are designed for reuse outside the product shell.
- `service` and `mode` are often equivalent in UX, but the config object is named `service` in code and storage.
- Test files can be large; implementation size alone is not the best signal of architectural complexity.

## Fast Search Checklist For AI Agents

If you need answers quickly, search in this order:

1. `project memory`
2. `sync_core_pools`
3. `HostContext`
4. `UniGatewayEngine`
5. `ExecutionTarget`
6. `gateway/support/execution_flow`
7. `provider hint`
8. `gateway api key`

## Suggested First Files To Read In Code

If you are about to modify behavior, start here:

1. `src/server.rs`
2. `src/types.rs`
3. `src/gateway/support/request_flow.rs`
4. `src/gateway/support/execution_flow.rs`
5. `src/config/core_sync.rs`
6. `unigateway-host/src/host.rs`
7. `unigateway-host/src/core/mod.rs`
8. `unigateway-core/src/engine/mod.rs`
9. `unigateway-core/src/protocol/mod.rs`