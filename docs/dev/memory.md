# UniGateway Project Memory

This document is optimized for contributors and AI agents that need a fast but accurate mental model of the current UniGateway codebase.

## One-Screen Summary

UniGateway is a local-first LLM gateway with a CLI-first product shell, a reusable runtime bridge, and a reusable core execution engine.

The repository currently has three main layers:

1. Product shell in `src/`
   - HTTP server, CLI, config persistence, admin API, gateway authentication, telemetry, and user-facing workflows.
2. Runtime bridge in `unigateway-runtime/`
   - Converts product-level state into a stable host contract and translates core results into OpenAI / Anthropic-compatible HTTP responses.
3. Core execution engine in `unigateway-core/`
   - Manages provider pools, endpoint selection, retry / fallback policy, driver execution, streaming completion, and request reports.

The most important architectural shift is this:

- Old mental model: gateway handlers directly parse payloads, route to providers, and call upstreams.
- Current mental model: product shell prepares requests, runtime resolves execution targets, and `unigateway-core` performs the actual provider execution.

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
- config file loading / mutation / persistence
- admin API
- gateway authentication and request limits
- state assembly and core engine lifecycle

Key files:

- `src/main.rs`
  - CLI entry point and top-level command tree.
- `src/server.rs`
  - HTTP server startup, route registration, app state wiring, and background config-to-core sync trigger.
- `src/types.rs`
  - `AppConfig` and `AppState`.
- `src/config.rs`
  - In-memory gateway config state and runtime bookkeeping.
- `src/config/store.rs`
  - Load and persist TOML-backed config state.
- `src/config/core_sync.rs`
  - Projects config services/providers/bindings into `unigateway-core::ProviderPool` values.
- `src/middleware.rs`
  - Gateway key auth, quota, QPS, concurrency limits.
- `src/gateway.rs`
  - Thin HTTP handlers only.
- `src/gateway/support/request_flow.rs`
  - Build `RuntimeContext`, extract token / provider hint, authenticate, and parse typed request.
- `src/gateway/support/execution_flow.rs`
  - Main bridge from prepared request into runtime/core execution.

### `unigateway-runtime/`: runtime bridge

Main responsibilities:

- Define a stable host contract between product shell and reusable runtime logic.
- Turn pool-based execution into OpenAI / Anthropic-compatible HTTP responses.
- Provide env-fallback flow helpers and status mapping.

Key files:

- `unigateway-runtime/src/host.rs`
  - Defines `RuntimeContext` and the host traits:
    - `RuntimeConfigHost`
    - `RuntimeEngineHost`
    - `RuntimePoolHost`
    - `RuntimeRoutingHost`
- `unigateway-runtime/src/core/mod.rs`
  - Re-exports runtime execution entry points.
- `unigateway-runtime/src/core/chat/mod.rs`
  - OpenAI / Anthropic chat response translation and chat execution wrappers.
- `unigateway-runtime/src/core/chat/streaming.rs`
  - Streaming adapters, especially Anthropic-to-OpenAI SSE conversion logic.
- `unigateway-runtime/src/core/responses.rs`
  - OpenAI Responses API runtime handling and stream compatibility fallback.
- `unigateway-runtime/src/core/embeddings.rs`
  - Embeddings execution wrapper.
- `unigateway-runtime/src/core/targeting.rs`
  - Build `ExecutionTarget`, env pools, and provider-hint matching.
- `unigateway-runtime/src/flow.rs`
  - Common runtime flow combinators and environment credential fallback helpers.

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

Defined in `src/config.rs`.

Responsibilities:

- Own the parsed TOML config file.
- Track runtime quota / rate state.
- Track round-robin service counters for legacy shell logic.
- Mark dirty state and persist changes.
- Trigger background sync into the core engine.

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

### `RuntimeContext`

Defined in `unigateway-runtime/src/host.rs`.

Responsibilities:

- Present a stable interface to runtime logic.
- Decouple runtime crate logic from the product shell's concrete `AppState` type.

This is a major architectural boundary. Runtime code should rely on traits and host capabilities rather than directly reaching into product-specific state.

## Startup Lifecycle

Current startup path:

1. `src/main.rs`
   - Parse CLI flags.
   - Build `AppConfig`.
2. `src/server.rs`
   - Load `GatewayState` from config file.
   - Construct `AppState`, which also constructs `UniGatewayEngine` with built-in HTTP drivers and telemetry hooks.
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
   - metadata such as provider name, source endpoint id, provider family, and binding priority.
4. Unsupported or invalid services are skipped or removed from core sync.
5. Config-managed pools are marked with metadata:
   - `managed_by = gateway-config`

Important implications:

- If a service has no enabled providers, the pool is not executable.
- If a provider has no API key or cannot resolve its upstream, core sync rejects it.
- If a config-managed service disappears, its corresponding engine pool is removed.

## Request Lifecycle

### OpenAI / Anthropic request path

1. Route entry in `src/server.rs`
   - `POST /v1/chat/completions`
   - `POST /v1/responses`
   - `POST /v1/embeddings`
   - `POST /v1/messages`
2. Thin handler in `src/gateway.rs`
3. Request preparation in `src/gateway/support/request_flow.rs`
   - Build `RuntimeContext`
   - Extract token
   - Extract provider hint from headers / payload
   - Authenticate gateway key if present
   - Parse payload into typed request
4. Execution dispatch in `src/gateway/support/execution_flow.rs`
   - If gateway key matched: route by `service_id`
   - Otherwise: use environment fallback credentials
5. Runtime wrapper in `unigateway-runtime/src/core/*`
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
- `src/api_key.rs`

### Config and persistence

- `src/config.rs`
- `src/config/store.rs`
- `src/config/core_sync.rs`
- `src/config/schema.rs`

### Runtime bridge

- `src/runtime_host_adapter.rs`
- `unigateway-runtime/src/host.rs`
- `unigateway-runtime/src/core/*`
- `unigateway-runtime/src/flow.rs`

### Core execution

- `unigateway-core/src/engine/*`
- `unigateway-core/src/routing.rs`
- `unigateway-core/src/protocol/*`
- `unigateway-core/src/transport.rs`

### Product CLI / UX

- `src/main.rs`
- `src/cli.rs`
- `src/cli/`

## Common Extension Tasks

### Add a new HTTP endpoint

Likely touch:

1. `src/server.rs`
2. `src/gateway.rs`
3. `src/gateway/support/*`
4. `unigateway-runtime/src/core/*` if it needs a new runtime translation path
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
2. `unigateway-runtime/src/core/targeting.rs`
3. `unigateway-core/src/routing.rs`
4. `unigateway-core/src/engine/reporting.rs` if retry semantics change

## Known Non-Obvious Details

- The current [`docs/design/arch.md`](../design/arch.md) should describe the three-layer model, not the older direct gateway-to-upstream mental model.
- `AppState` implements the runtime host traits through the runtime host adapter path, even though runtime code only sees `RuntimeContext`.
- Env fallback is still a first-class path for requests without gateway auth.
- Runtime and core crates are designed for reuse outside the product shell.
- `service` and `mode` are often equivalent in UX, but the config object is named `service` in code and storage.
- Test files can be large; implementation size alone is not the best signal of architectural complexity.

## Fast Search Checklist For AI Agents

If you need answers quickly, search in this order:

1. `project memory`
2. `sync_core_pools`
3. `RuntimeContext`
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
6. `unigateway-runtime/src/host.rs`
7. `unigateway-runtime/src/core/mod.rs`
8. `unigateway-core/src/engine/mod.rs`
9. `unigateway-core/src/protocol/mod.rs`