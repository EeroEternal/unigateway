# UniGateway Project Memory

This document is optimized for contributors and AI agents that need a fast but accurate mental model of the current UniGateway codebase.

> **Workspace note (current):** This repository ships **library crates only** (`unigateway-core`, `unigateway-protocol`, `unigateway-host`, `unigateway-config`, `unigateway-sdk`). The former `ug` binary, `src/` HTTP/CLI product shell, `AppState`, and `unigateway-cli` are **removed**. Sections below that still mention `src/*`, CLI commands, or admin HTTP routes describe the **previous** product layout; treat them as historical context unless you are porting behavior into your own gateway.

## One-Screen Summary

UniGateway is a local-first LLM **library workspace**: config state, host bridge, protocol translation, core execution engine, and a thin `unigateway-sdk` facade. Your application owns HTTP, auth, admin, process lifecycle, and user management.

The repository currently has five main layers:

1. Embedder facade in `unigateway-sdk/`
  - Re-exports `unigateway-core`, `unigateway-protocol`, and `unigateway-host` under a single namespaced dependency without adding a second abstraction layer.
2. Config state in `unigateway-config/`
  - TOML-backed `GatewayState`, mutation helpers, routing helpers, and config → core pool projection.
3. Host bridge in `unigateway-host/`
  - Converts embedder-supplied context into a stable host contract, exposes a unified dispatch API, returns typed `HostError` values, and translates core results into protocol-owned neutral HTTP response payloads.
4. Core execution engine in `unigateway-core/`
  - Manages provider pools, endpoint selection, retry / fallback policy, driver execution, streaming completion, and request reports.
5. Protocol surface in `unigateway-protocol/`
  - Request parsing, response formatting, and neutral HTTP response types consumed by embedders.

The most important architectural shift is this:

- Old mental model: monolithic gateway handlers directly parse payloads, route to providers, and call upstreams.
- Current mental model: embedder prepares `HostContext` and request envelopes, host-layer code resolves execution targets, and `unigateway-core` performs the actual provider execution.
- Embedder entry model: external applications should usually start from `unigateway-sdk`, then reach through to `core`, `protocol`, and `host` namespaces as needed.

## Product Identity

UniGateway libraries are intended to power a **stable entry point** between AI tools and multiple upstream model providers inside **your** host binary or service.

Primary goals:

- One routing abstraction for switching between upstream providers: `mode` / `service`.
- Reliable failover / fallback centralized in the core engine.
- TOML-backed config state with projection into executable pools.
- Typed host errors and neutral protocol responses so embedders can map to their HTTP stack.

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

### Former root product shell (removed)

The historical `ug` binary lived in a root crate under `src/` together with `unigateway-cli`. That HTTP/CLI/admin/MCP surface is **no longer in this repository**. Implement equivalent wiring (config load, `UniGatewayEngine`, pool sync, HTTP auth, admin routes) in your own application using the library crates.

### `unigateway-config/`: config state crate

Main responsibilities:

- Own TOML-backed gateway config state and persistence.
- Provide admin / programmatic mutation helpers over `GatewayState`.
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

- Define a stable host contract between embedder wiring and reusable host logic.
- Delegate protocol parsing and neutral HTTP response shaping to `unigateway-protocol`.
- Materialize env-backed fallback pools through the host boundary.
- Expose unified dispatch over chat / responses / embeddings.
- Return typed host errors while leaving HTTP response adaptation to the embedder.

Key files:

- `unigateway-host/src/host.rs`
  - Defines `HostContext`, `PoolHost`, and explicit `PoolLookupOutcome` values for host-side pool resolution.
- `unigateway-host/src/error.rs`
  - Defines typed `HostError` / `HostResult` for dispatch mismatch, pool lookup, targeting, and core execution failures.
- `unigateway-host/src/env.rs`
  - Defines `EnvProvider`, `EnvPoolHost`, and env-backed fallback helpers for embedders.
- `unigateway-protocol/src/lib.rs`
  - Re-exports protocol request parsers, response renderers, and neutral response types.
- `unigateway-protocol/src/requests.rs`
  - JSON payload to `Proxy*Request` translation.
- `unigateway-protocol/src/responses.rs`
  - `ProxySession` and completed response to the neutral protocol response type `ProtocolHttpResponse`, including SSE shaping.
- `unigateway-protocol/src/http_response.rs`
  - Neutral HTTP response body and streaming types shared by protocol rendering and embedders.
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
  - Map typed `HostError` values to HTTP status codes for embedders.

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

## Primary state handles

### `GatewayState`

Defined in `unigateway-config/src/lib.rs`.

Responsibilities:

- Compose config-facing and runtime-facing sub-state for the gateway.
- Own the parsed TOML config file.
- Track runtime quota / rate state.
- Mark dirty state and persist changes.
- Trigger background sync into the core engine.
- Expose focused read/write helpers so embedder code does not lock `inner` / `api_key_runtime` directly.

Current shape:

- `ConfigStore` owns the TOML-backed file state, dirty bit, and core-sync notifier.
- `RuntimeRateLimiter` owns per-key qps/concurrency tokens and queue bookkeeping.

Persistence model:

- Config file contents are durable.
- Runtime request counters and in-flight bookkeeping are memory-only.

### `HostContext`

Defined in `unigateway-host/src/host.rs`.

Responsibilities:

- Present a stable interface to host-layer logic (`PoolHost`, engine handle, optional env pool materialization).
- Keep host code independent of your application’s concrete app-state type: construct whatever structs you need, then build `HostContext` per request or share an `Arc` wrapper as appropriate.

Historical note: the old monolithic repo split request/admin/system state across `src/types.rs` and `src/admin/*`. Those types are gone; mirror the responsibilities in your gateway if you need the same route surfaces.

## Startup lifecycle (embedder)

Typical sequence in **your** binary:

1. Load or create `GatewayState` (`unigateway-config`) from disk.
2. Build `UniGatewayEngine` and register drivers / hooks as needed.
3. Call `GatewayState::sync_core_pools` (or equivalent) so config-managed pools exist in the engine.
4. On config mutations, use the config store notifier pattern to re-sync pools and persist dirty state on an interval or explicit flush.

Important consequence:

- The config file is not the live execution source of truth; **`UniGatewayEngine`** holds executable pools after projection.

## Config projection: service → pool

The transformation lives in **`unigateway-config/src/core_sync.rs`**.

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

## Request lifecycle (embedder)

1. **Your HTTP/router** receives the request (OpenAI or Anthropic path, health, metrics, admin — as you implement).
2. **Auth / limits** — validate gateway keys, quotas, QPS, concurrency using `GatewayState` (`select`, `runtime`).
3. **Protocol parse** — `unigateway-protocol` builds typed proxy requests.
4. **Host dispatch** — `unigateway-host` builds `ExecutionTarget`, runs the engine, returns `HostError` or neutral `ProtocolHttpResponse` / streaming session.
5. **Core** — retries, fallback, drivers, streaming normalization.

### Authentication behavior (to implement in host)

Rules mirrored from the former product shell:

- If a gateway API key is present and valid, requests route through service-based execution.
- If no key matches, the request may fall back to environment-provided upstream credentials.
- Optional localhost shortcut: if listen address is local and exactly one active gateway API key exists, an empty token may authenticate as that key (only if you choose to preserve this UX).

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

Hooks can be attached to `UniGatewayEngine` via `GatewayHooks` from your application startup.

## File map by concern

### Request ingress (your application)

- HTTP router / handlers that call into protocol + host dispatch.

### Auth and limits

- `unigateway-config/src/select.rs` — API key lookup.
- `unigateway-config/src/runtime.rs` — QPS / concurrency / queue metrics.

### Admin-style mutations (optional in host)

- `unigateway-config/src/admin.rs` — shared mutation helpers; wire to your own `/api/admin/*` if desired.

### Config and persistence

- `unigateway-config/src/store.rs`
- `unigateway-config/src/core_sync.rs`
- `unigateway-config/src/schema.rs`
- `unigateway-config/src/runtime.rs`

### Host bridge

- `unigateway-host/src/host.rs`
- `unigateway-host/src/core/*`
- `unigateway-host/src/status.rs`

### Core execution

- `unigateway-core/src/engine/*`
- `unigateway-core/src/routing.rs`
- `unigateway-core/src/protocol/*`
- `unigateway-core/src/transport.rs`

## Common extension tasks

### Add a new HTTP endpoint

1. Your router / Axum (or other) module.
2. `unigateway-host/src/core/*` if a new host translation path is required.
3. `unigateway-core/src/protocol/*` if a new upstream protocol call is required.

### Add a new provider family

1. `unigateway-core/src/protocol/`
2. `unigateway-core/src/protocol/mod.rs`
3. `unigateway-config/src/core_sync.rs` for `provider_type -> driver_id / provider_kind` mapping
4. Protocol response shaping in `unigateway-protocol` if client compatibility needs it.

### Change service-to-provider selection behavior

1. `unigateway-config/src/core_sync.rs`
2. `unigateway-host/src/core/targeting.rs`
3. `unigateway-core/src/routing.rs`
4. `unigateway-core/src/engine/reporting.rs` if retry semantics change.

## Known Non-Obvious Details

- Read `[docs/design/arch.md](../design/arch.md)` for the current library-layer model.
- Env fallback remains a first-class path for requests without gateway auth when you wire `EnvPoolHost`.
- `service` and `mode` are often equivalent in UX, but the config object is named `service` in code and storage.
- Test files can be large; implementation size alone is not the best signal of architectural complexity.

## Fast Search Checklist For AI Agents

If you need answers quickly, search in this order:

1. `project memory`
2. `sync_core_pools`
3. `HostContext`
4. `UniGatewayEngine`
5. `ExecutionTarget`
6. `dispatch_request`
7. `provider hint`
8. `gateway api key`

## Suggested First Files To Read In Code

If you are about to modify behavior, start here:

1. `unigateway-config/src/lib.rs`
2. `unigateway-config/src/core_sync.rs`
3. `unigateway-host/src/host.rs`
4. `unigateway-host/src/core/dispatch.rs`
5. `unigateway-protocol/src/requests.rs`
6. `unigateway-protocol/src/responses.rs`
7. `unigateway-core/src/engine/mod.rs`
8. `unigateway-core/src/protocol/mod.rs`