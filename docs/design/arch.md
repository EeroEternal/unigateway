# UniGateway Architecture

This document describes the current architecture of the repository, including the split between the product shell, the extracted CLI crate, the reusable config state crate, the reusable host bridge, and the reusable execution engine.

For an AI-oriented high-signal summary, start with [`dev/memory.md`](../dev/memory.md).

## Current Architecture In One Sentence

UniGateway is a CLI-first local gateway whose product shell manages HTTP, auth, and UX, whose config crate owns persisted gateway state and core-pool projection, whose host layer translates prepared requests into execution targets, and whose core layer executes those requests against provider pools with retry, fallback, and protocol drivers.

## Top-Level Layers

### 1. Product shell (`src/`)

Responsibilities:

- binary glue around the extracted CLI crate
- HTTP server and route registration
- admin API
- gateway API key auth and runtime limits
- app-wide state assembly
- trigger config-to-core sync

Key files:

- `unigateway-cli/src/lib.rs`
- `unigateway-cli/src/setup.rs`
- `src/admin/mod.rs`
- `src/admin/mcp.rs`
- `src/main.rs`
- `src/server.rs`
- `src/types.rs`
- `src/middleware.rs`
- `src/gateway.rs`
- `src/gateway/support/request_flow.rs`
- `src/gateway/support/execution_flow.rs`

### 1.5. Config state (`unigateway-config/`)

Responsibilities:

- own TOML-backed gateway config state and persistence
- provide admin / CLI mutation and query helpers over `GatewayState`
- project services, providers, and bindings into `unigateway-core::ProviderPool`
- own config-scoped routing helpers like upstream URL resolution

Key files:

- `unigateway-config/src/lib.rs`
- `unigateway-config/src/store.rs`
- `unigateway-config/src/admin.rs`
- `unigateway-config/src/select.rs`
- `unigateway-config/src/core_sync.rs`
- `unigateway-config/src/routing.rs`

### 2. Host bridge (`unigateway-host/`)

Responsibilities:

- define the boundary between the product shell and reusable host logic
- expose `HostContext` and host traits
- build `ExecutionTarget`s for the core engine
- delegate protocol parsing and neutral HTTP response shaping to `unigateway-protocol`
- provide env-fallback and status-mapping helpers

Key files:

- `unigateway-host/src/host.rs`
- `unigateway-host/src/core/mod.rs`
- `unigateway-host/src/core/chat/`
- `unigateway-host/src/core/responses.rs`
- `unigateway-host/src/core/embeddings.rs`
- `unigateway-host/src/core/targeting.rs`
- `unigateway-host/src/flow.rs`

### 2.5. Protocol translation (`unigateway-protocol/`)

Responsibilities:

- parse OpenAI / Anthropic-compatible JSON payloads into `Proxy*Request`
- render `ProxySession` values into a neutral protocol response type (`RuntimeHttpResponse`, legacy name)
- shape SSE streams without depending on axum or the product shell

Key files:

- `unigateway-protocol/src/lib.rs`
- `unigateway-protocol/src/requests.rs`
- `unigateway-protocol/src/responses.rs`
- `unigateway-protocol/src/http_response.rs`

### 3. Core execution engine (`unigateway-core/`)

Responsibilities:

- store pools and endpoints in memory
- resolve targets into ordered attempts
- execute provider drivers
- apply retry policy and fallback behavior
- normalize streaming completion
- emit request / attempt reports and hooks

Key files:

- `unigateway-core/src/lib.rs`
- `unigateway-core/src/engine/mod.rs`
- `unigateway-core/src/engine/execution.rs`
- `unigateway-core/src/engine/reporting.rs`
- `unigateway-core/src/routing.rs`
- `unigateway-core/src/protocol/mod.rs`
- `unigateway-core/src/protocol/openai/`
- `unigateway-core/src/protocol/anthropic.rs`

## Core Vocabulary Mapping

The project uses similar terms at different layers.

- `mode`
  - user-facing routing choice
- `service`
  - persisted config object backing a mode
- `provider`
  - configured upstream entry in TOML
- `binding`
  - relation between service and provider
- `pool`
  - execution-time object stored in `UniGatewayEngine`
- `endpoint`
  - one upstream candidate inside a pool
- `ExecutionTarget`
  - what the core engine executes for a request
- `gateway api key`
  - UniGateway-managed key bound to a service
- `upstream api key`
  - actual provider credential

Most importantly:

- service -> pool
- provider + binding -> endpoint

## Startup And Sync Flow

Startup flow:

1. `src/main.rs`
   - parse CLI and build `AppConfig`
2. `src/server.rs`
   - load `GatewayState`
   - build `AppState`
  - derive `SystemState`, `GatewayRequestState`, and `AdminState`
   - create `UniGatewayEngine`
   - register core sync notifier
   - call `sync_core_pools()`
3. background sync task in `src/server.rs`
   - listens for config mutation notifications
   - rebuilds core pools after config changes
4. background persistence task in `src/server.rs`
   - periodically persists dirty config state to disk

This means the executable state for requests lives in memory inside `UniGatewayEngine`, not directly in the TOML file.

## Config Projection To Core Pools

Config projection happens in `src/config/core_sync.rs`.

Rules:

1. Each service becomes one `ProviderPool`.
2. Each binding contributes a provider candidate.
3. Each provider becomes a core `Endpoint` with:
   - `driver_id`
   - `provider_kind`
   - resolved `base_url`
   - provider API key
   - parsed `ModelPolicy`
   - metadata such as provider family and binding priority
4. Invalid services are skipped or removed from the engine.
5. Config-managed pools are tagged with `managed_by = gateway-config`.

This projection is the bridge between the product config model and the reusable core execution model.

## Request Path

System request path:

1. HTTP route in `src/server.rs`
2. thin handler in `src/system.rs`
  - runs with `SystemState`

Current request path:

1. HTTP route in `src/server.rs`
2. thin handler in `src/gateway.rs`
  - runs with `GatewayRequestState`
3. request preparation in `src/gateway/support/request_flow.rs`
   - build `HostContext`
   - extract token
   - extract provider hint
   - authenticate gateway key if present
   - parse typed request
4. dispatch in `src/gateway/support/execution_flow.rs`
   - route by service if gateway auth succeeded
   - otherwise use environment-based upstream fallback
5. host execution wrapper in `unigateway-host/src/core/*`
   - build `ExecutionTarget`
   - call `UniGatewayEngine`
   - translate result into external protocol response
6. core execution in `unigateway-core`
   - select endpoints
   - execute drivers
   - retry / fallback
   - stream completion
   - request report

## Authentication And Fallback Behavior

Authentication is handled in `src/middleware.rs`.

Important rules:

- gateway keys route requests by `service_id`
- env fallback remains supported for requests without a matching gateway key
- localhost compatibility shortcut exists:
  - if bind is local and exactly one active gateway key exists, an empty token can implicitly authenticate as that key

This behavior matters for local tool integrations and AI clients.

## Concurrency and Queuing

To protect upstream providers and ensure fair allocation across API keys, UniGateway implements an asynchronous queue mechanism in `src/middleware.rs`. Rather than aggressively returning a 429 error when the `concurrency_limit` is exceeded, the server suspends the request via a token buffer limit until a spot opens up. 

For a detailed walkthrough on how backpressure and queue limits are safely evaluated, see [`queue.md`](./queue.md).

## Host Boundary

`unigateway-host/src/host.rs` defines the host contract.

Host traits:

- `EngineHost`
- `PoolHost`

Key host-side routing types:

- `HostPoolSource`
- `HostEnvProvider`

`HostContext` is the stable view host-layer code receives.

Design intent:

- host-layer code should not depend on the concrete product `AppState`
- gateway request handling should depend on `GatewayRequestState`, not the full product-shell state
- host applications should provide engine access plus pool resolution through traits
- env fallback should be materialized via `PoolHost::env_pool`, not via provider-specific config fields on the host context
- later extraction or reuse should mostly be file movement, not API redesign

## Core Engine Model

`UniGatewayEngine` is the authoritative execution object.

It owns:

- pool registry
- round-robin counters
- hook installation
- driver registry
- default retry policy
- optional default timeout

Execution is currently split into:

- `engine/mod.rs`
  - state and builder
- `engine/execution.rs`
  - chat / responses / embeddings attempt loops
- `engine/reporting.rs`
  - retry conditions, attempt reports, hook emission, streaming finalization

## Protocol Drivers

Built-in drivers currently include:

- OpenAI-compatible
- Anthropic

Driver responsibilities:

- build upstream HTTP requests
- parse non-streaming responses
- normalize streaming frames into internal chunk / event types

Runtime responsibilities are different:

- turn internal normalized results back into client-facing OpenAI / Anthropic responses

That split is central to the current architecture.

## Streaming Model

Streaming is normalized in core and adapted in runtime.

Core side:

- `ProxySession::Streaming`
- `StreamingResponse`
- typed chunks / events
- completion handle for final report

Runtime side:

- OpenAI-compatible SSE output
- Anthropic SSE compatibility generation
- stream-to-non-stream fallback for some responses flows

This split keeps provider-specific parsing in core and protocol-specific client response shaping in runtime.

## Design Principles

- Local-first and CLI-first
- Single process and file-backed config state
- Reusable core and runtime crates
- Clear boundary between product semantics and execution semantics
- Explainable request routing and observable execution
- Small, composable protocol / runtime / engine modules over monolithic handlers

## Where To Start Reading

If you are changing behavior, start here:

1. `src/server.rs`
2. `src/types.rs`
3. `src/gateway/support/request_flow.rs`
4. `src/gateway/support/execution_flow.rs`
5. `src/config/core_sync.rs`
6. `unigateway-host/src/host.rs`
7. `unigateway-host/src/core/mod.rs`
8. `unigateway-core/src/engine/mod.rs`
9. `unigateway-core/src/protocol/mod.rs`
