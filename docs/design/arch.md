# UniGateway Architecture

This document describes the **library workspace** in this repository: `unigateway-sdk`, `unigateway-config`, `unigateway-host`, `unigateway-protocol`, and `unigateway-core`. There is no in-tree HTTP server or CLI; your application provides routes, auth, admin, and process lifecycle.

For a contributor-oriented summary, read [`dev/memory.md`](../dev/memory.md).

## One-Sentence Model

Embedders load TOML-backed `GatewayState`, project it into `UniGatewayEngine` pools, build `HostContext`, run `unigateway-host` dispatch on typed requests parsed by `unigateway-protocol`, and map `ProtocolHttpResponse` / streaming outcomes onto their own HTTP stack.

## Layers

### 1. Embedder facade (`unigateway-sdk/`)

- Single dependency entry for `unigateway_sdk::core`, `::protocol`, `::host`.
- Feature flags: `core`, `protocol`, `host`, `embed`, `testing`.

### 2. Config state (`unigateway-config/`)

- Owns `GatewayState`, persistence, mutations, API-key runtime limits (`runtime.rs`), and **config → core pool** projection (`core_sync.rs`).
- Upstream URL helpers live in `routing.rs`.

### 3. Host bridge (`unigateway-host/`)

- Stable contract: `HostContext`, `PoolHost`, `EnvPoolHost`, dispatch in `core/dispatch.rs`.
- Builds `ExecutionTarget`, calls the engine, returns typed `HostError` and neutral protocol payloads via `unigateway-protocol`.

### 4. Protocol translation (`unigateway-protocol/`)

- Parses OpenAI / Anthropic-shaped JSON into internal proxy request types.
- Renders `ProxySession` into `ProtocolHttpResponse` and SSE-friendly streaming types (`http_response.rs`).
- No Axum dependency.

### 5. Core execution engine (`unigateway-core/`)

- In-memory pools, endpoint ordering, drivers (`protocol/openai`, `protocol/anthropic`), retry/fallback, streaming normalization, hooks.

## Config projection (service → pool)

Implemented in **`unigateway-config/src/core_sync.rs`** (not under `src/`).

Rules (summary):

1. Each service → one `ProviderPool`.
2. Each binding → one candidate endpoint ordering contribution.
3. Each provider → `Endpoint` with `driver_id`, `provider_kind`, resolved `base_url`, credentials, `ModelPolicy`, metadata; invalid rows skipped.
4. Config-managed pools carry `managed_by = gateway-config`.

The TOML file is durable; executable truth for routing is **`UniGatewayEngine`** after `sync_core_pools()`.

## Request path (logical)

What used to live in `src/gateway/*` is now **your** HTTP layer’s responsibility. The library path looks like:

1. **Ingress (your code)** — route, read headers/body, optional gateway-key auth and per-key limits via `GatewayState` helpers (`unigateway-config/src/runtime.rs`, `select.rs`).
2. **Parse** — `unigateway-protocol` request parsers.
3. **Host dispatch** — `unigateway-host` builds targets, runs `UniGatewayEngine`, maps results to `ProtocolHttpResponse` or streaming session.
4. **Core** — drivers, retries, fallback, reports.

Auth semantics (gateway key vs env fallback, localhost single-key shortcut) are **design expectations** you may reimplement; see [`admin.md`](./admin.md) for admin JSON contracts if you expose `/api/admin/*`.

## Concurrency and queuing

Per-key QPS/concurrency and queue metrics are implemented in **`unigateway-config`** (`runtime.rs`). A host server should acquire/release slots around upstream execution. Design background: [`queue.md`](./queue.md).

## Host boundary

`unigateway-host/src/host.rs` defines `HostContext` and pool resolution traits. Host code must not depend on a removed `AppState`; pass only what dispatch needs (engine, pool hosts, optional env pool).

## Core engine, drivers, streaming

Same as before: see `unigateway-core/src/engine/*` and `protocol/mod.rs`. Streaming is normalized in core and adapted to client SSE shapes in protocol rendering.

## Principles

- **Library-first**: no bundled gateway binary in this repo.
- **Clear boundary**: config + host + protocol + core vs your HTTP/admin UX.
- **Observable execution**: hooks and reports on `UniGatewayEngine`.

## Where to start reading code

1. `unigateway-config/src/lib.rs` — `GatewayState`, sync entrypoints.
2. `unigateway-config/src/core_sync.rs` — projection rules.
3. `unigateway-host/src/host.rs` — `HostContext` contract.
4. `unigateway-host/src/core/dispatch.rs` — dispatch funnel.
5. `unigateway-protocol/src/requests.rs` / `responses.rs`
6. `unigateway-core/src/engine/mod.rs` — engine lifecycle.

Integration guide: [`../guide/embed.md`](../guide/embed.md).
