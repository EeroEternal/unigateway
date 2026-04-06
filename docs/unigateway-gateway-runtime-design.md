# RFC: UniGateway Gateway Runtime Layer

Status: Draft

Date: 2026-04-06

Authors: UniGateway maintainers

Related documents:

- [docs/unigateway-core-api-draft.md](docs/unigateway-core-api-draft.md)
- [docs/unigateway-core-implementation-plan.md](docs/unigateway-core-implementation-plan.md)

## 1. Summary

This document defines the next architecture layer above `unigateway-core`.

The repository should not stop at a reusable execution engine. It should also provide a reusable gateway runtime layer that owns HTTP protocol compatibility, request normalization, response shaping, streaming wire output, and controlled fallback behavior.

That layer is intended for embedders such as OpenHub that need a complete gateway runtime, but do not want the current UniGateway product shell with its local config, admin, CLI, and operational behavior.

## 2. Problem Statement

`unigateway-core` solves only part of the reuse problem.

It is the correct place for:

- in-memory pools and snapshots
- driver execution
- retry and failover inside the execution engine
- streaming and completed response models

However, OpenHub-style embedding may still need more than that:

- OpenAI-compatible HTTP endpoints
- Anthropic-compatible HTTP endpoints
- request payload normalization from wire JSON into runtime requests
- SSE shaping for downstream clients
- compatibility handling for incomplete or legacy upstream behaviors

Those concerns do not belong in `unigateway-core`, but they also should not remain trapped inside the current end-user UniGateway binary.

## 3. Target Packaging Model

The target repository shape should evolve toward four layers.

```text
unigateway-core/
  reusable execution engine

unigateway-runtime/
  reusable gateway runtime

unigateway/
  product shell, local config, admin, CLI, MCP, process management

OpenHub/
  external embedding application using the first two layers
```

The package name for the reusable gateway runtime should be `unigateway-runtime`.

## 4. Layer Responsibilities

### 4.1 `unigateway-core`

`unigateway-core` should remain responsible for:

- pure in-memory runtime state
- pool upsert, remove, and snapshot semantics
- driver registry and built-in drivers
- execution by pool or execution plan
- execution-time retry and failover
- standardized chat, responses, and embeddings result types

`unigateway-core` should not become an HTTP compatibility crate.

### 4.2 Gateway Runtime Layer

The gateway runtime layer should be responsible for:

- OpenAI-compatible and Anthropic-compatible HTTP semantics
- request JSON normalization into runtime request types
- response shaping back into protocol-compatible JSON and SSE
- translating product or host-level routing context into `ExecutionTarget`
- bridging normalized requests into `unigateway-core`
- narrowly scoped compatibility fallback for behaviors that are intentionally outside core

This layer is the reusable HTTP-facing runtime.

### 4.3 UniGateway Product Shell

The current `unigateway` binary should eventually become a thin product shell around the gateway runtime layer.

It should remain responsible for:

- local config file lifecycle
- admin APIs
- gateway API key management
- auth and quota middleware
- MCP operations
- CLI flows and diagnostics
- local process management

### 4.4 OpenHub or Other Embedders

An external host such as OpenHub should be able to:

- own authentication and billing
- own database-backed control plane
- own tenant and account logic
- materialize pool snapshots or execution plans
- reuse the gateway runtime for HTTP compatibility and protocol shaping
- reuse `unigateway-core` for execution

## 5. Gateway Runtime Boundary

The gateway runtime layer should be intentionally narrow.

### 5.1 In Scope

- HTTP request parsing for OpenAI-compatible and Anthropic-compatible endpoints
- request-to-core conversion
- core execution bridging
- HTTP response formatting
- SSE event formatting for downstream clients
- compatibility fallback that exists only because some upstreams are not fully uniform yet

### 5.2 Out of Scope

- config-file persistence
- admin CRUD endpoints
- API key storage and lifecycle
- rate limiting and billing enforcement
- tenant management
- CLI rendering and operational commands
- process supervision

## 6. Internal Sub-Layers of the Gateway Runtime

To stay reusable, the gateway runtime should itself be layered.

### 6.1 Protocol Compatibility Layer

Responsibilities:

- parse HTTP payloads into normalized request types
- translate runtime outputs into OpenAI or Anthropic wire-compatible JSON
- generate SSE output for streaming responses

Current code that already fits this layer:

- `src/protocol.rs`
- parts of `src/gateway/core_adapter.rs`
- parts of `src/gateway/streaming.rs`

### 6.2 Core Bridge Layer

Responsibilities:

- accept normalized requests plus routing context
- select a pool or execution plan
- upsert runtime pools into `unigateway-core`
- call the engine
- convert the engine result into protocol-layer outputs

Current code that already fits this layer:

- `src/gateway/core_bridge.rs`
- `src/config/core_sync.rs`

### 6.3 Compatibility Strategy Layer

Responsibilities:

- isolate temporary or provider-specific fallback behavior that should not enter the core crate
- keep legacy interoperability code out of handlers and out of product admin/config code

Current code that already fits this layer:

- `src/gateway/responses_compat.rs`

### 6.4 Optional HTTP Framework Adapter Layer

Responsibilities:

- bind the runtime into Axum routes, or another framework if desired later
- construct handler state from host-provided dependencies

This sub-layer should stay thin so the reusable gateway runtime is not tightly coupled to a single application shell.

## 7. Proposed Runtime API Direction

The initial extraction does not need a perfect public API, but the direction should be explicit.

The gateway runtime should move toward host-supplied interfaces such as:

- a runtime state object with access to `unigateway-core`
- a host-provided pool resolver or pool sync callback
- a host-provided auth result or request context object
- handler entry points or router builders for OpenAI and Anthropic compatibility

The important point is inversion of control:

- the host supplies auth and routing context
- the gateway runtime supplies protocol compatibility and execution behavior

## 8. Current Code Mapping to the Future Runtime Layer

The following current modules should be considered candidates for the new reusable gateway runtime package.

| Current module | Future home | Reason |
| --- | --- | --- |
| `src/protocol.rs` | gateway runtime | HTTP request normalization and protocol shaping |
| `src/protocol/messages.rs` | gateway runtime | wire-format request parsing |
| `src/gateway/core_adapter.rs` | gateway runtime | request and response conversion |
| `src/gateway/core_bridge.rs` | gateway runtime | execution bridge into core |
| `src/gateway/responses_compat.rs` | gateway runtime | temporary compatibility fallback |
| parts of `src/gateway/streaming.rs` | gateway runtime | SSE shaping and stream forwarding |
| parts of `src/gateway/chat.rs` | gateway runtime | protocol-specific downstream response wiring |

The following modules should remain product-only.

| Current module | Stay in product shell | Reason |
| --- | --- | --- |
| `src/middleware.rs` | yes | auth, quota, and stats are product concerns |
| `src/api_key.rs` | yes | API key lifecycle is not runtime reuse |
| `src/service.rs` | yes | admin CRUD |
| `src/provider.rs` | yes | admin CRUD |
| `src/mcp.rs` | yes | operations and daemon control |
| `src/cli/*` | yes | product UX, not gateway runtime |
| `src/config/*` persistence logic | yes | local config lifecycle |

## 9. Migration Order

The migration should avoid a rewrite. The recommended order is below.

### Step 1: Freeze the Runtime Boundary in Docs

Deliverables:

- define the gateway runtime as a layer distinct from both `unigateway-core` and the UniGateway product shell
- agree that OpenHub should target this reusable layer rather than the full product binary

Exit criteria:

- no ambiguity about what belongs in core versus runtime versus product shell

### Step 2: Finish Handler Thinning Inside the Current Crate

Deliverables:

- move remaining protocol execution and compatibility branches out of `src/gateway.rs`
- keep handlers limited to parse, authenticate, delegate, and return

Current status:

- already in progress with `core_adapter`, `core_bridge`, and `responses_compat`

Exit criteria:

- `src/gateway.rs` contains no provider loops beyond runtime delegation

### Step 3: Consolidate Runtime-Owned Modules Under a Single Internal Boundary

Deliverables:

- gather protocol normalization, bridge, stream shaping, and compat code behind one internal runtime namespace
- reduce direct dependencies from handlers into scattered helper modules

Suggested source moves:

- `src/protocol.rs`
- `src/gateway/core_adapter.rs`
- `src/gateway/core_bridge.rs`
- `src/gateway/responses_compat.rs`
- runtime-relevant parts of `src/gateway/chat.rs` and `src/gateway/streaming.rs`

Exit criteria:

- product handlers use a coherent runtime-facing API instead of reaching many helper files directly

### Step 4: Introduce Host-Owned Runtime Context Interfaces

Deliverables:

- replace direct dependence on `AppState` where practical with narrower runtime-facing context traits or structs
- isolate host responsibilities such as auth result, pool resolution, and config lookup

Exit criteria:

- runtime modules no longer assume the full UniGateway product state object

### Step 5: Extract a New Crate for the Gateway Runtime

Deliverables:

- create `unigateway-runtime`
- move runtime-owned modules into the new crate
- keep Axum-specific route binding either in the new crate or in a very thin wrapper next to it

Exit criteria:

- the UniGateway product shell depends on the new runtime crate instead of owning those modules inline

### Step 6: Rebuild the UniGateway Product Shell on Top of the Runtime

Deliverables:

- keep local config, admin, CLI, MCP, and process behavior in the product binary
- route requests through the reusable runtime layer

Exit criteria:

- the end-user binary becomes a composition of product concerns plus runtime crate plus core crate

### Step 7: Add Embedder Documentation and Reference Integration

Deliverables:

- document how a host such as OpenHub can supply auth, routing context, and pool snapshots
- provide one minimal embedder example without UniGateway-specific config persistence

Exit criteria:

- an external host can integrate the runtime without depending on product shell code

## 10. Design Constraints

The gateway runtime extraction should follow these rules.

1. Do not move local config persistence into the runtime layer.
2. Do not move billing or quota logic into the runtime layer.
3. Do not widen `unigateway-core` to absorb HTTP wire concerns.
4. Do not let temporary compatibility fallback leak back into handlers.
5. Keep OpenAI-compatible and Anthropic-compatible behavior as the main reusable protocol surface.

## 11. Success Criteria

This gateway runtime extraction should be considered successful when all of the following are true.

1. `unigateway-core` remains clean and database-agnostic.
2. HTTP protocol compatibility no longer lives only inside the product shell.
3. the UniGateway binary becomes a thin composition layer over runtime plus product concerns.
4. OpenHub can reuse a complete gateway runtime without adopting UniGateway local admin and CLI behavior.
5. remaining compatibility fallback is isolated in a clearly bounded runtime module.

## 12. Immediate Next Coding Steps

The next implementation steps should be:

1. continue shrinking `src/gateway.rs` until handlers only delegate into the runtime-facing modules
2. consolidate any remaining stream-shaping code that is still split between handlers and helper files
3. reduce direct runtime dependence on `AppState` by introducing narrower runtime context inputs
4. only then extract a new crate boundary for the gateway runtime