# UniGateway Core Implementation Plan

Status: In Progress

Date: 2026-04-06

Related RFC: [docs/unigateway-core-api-draft.md](docs/unigateway-core-api-draft.md)

Gateway runtime layering: [docs/unigateway-gateway-runtime-design.md](docs/unigateway-gateway-runtime-design.md)

Phase 0 checklist: [docs/unigateway-core-phase0-checklist.md](docs/unigateway-core-phase0-checklist.md)

Phase 1 task sheet: [docs/unigateway-core-phase1-task-sheet.md](docs/unigateway-core-phase1-task-sheet.md)

## 1. Purpose

This document translates the `unigateway-core` RFC into a concrete development plan.

It is not an API specification. It is an execution plan for migrating the current repository from a product-oriented single binary into a layered architecture with a reusable core crate.

The primary goal is to reach the RFC target with controlled, reviewable steps instead of a full rewrite.

## 2. Current-State Assessment

The current codebase is already partially layered, but the execution path is still tightly coupled to product concerns.

### 2.1 Current Coupling Points

The main execution path is currently spread across:

- `src/server.rs`: HTTP route registration and application startup
- `src/gateway.rs`: request handlers, provider loops, auth integration, fallback execution
- `src/middleware.rs`: gateway API key auth, quota checks, runtime rate limits, request stats
- `src/routing.rs`: provider selection and protocol hinting
- `src/config/*`: runtime state, admin mutations, persistence to config file
- `src/protocol/*`: request conversion and upstream invocation via `llm-connector`

### 2.2 Key Architectural Mismatches Against the RFC

The current implementation diverges from the target RFC in the following ways:

1. Runtime execution depends on product-owned auth and quota concerns.
2. Runtime state and config-file persistence are stored in the same subsystem.
3. The HTTP layer is not yet a thin adapter; it still owns execution orchestration.
4. Upstream protocol execution depends on `llm-connector`.
5. Admin APIs and CLI flows mutate the same state object used by request execution.

### 2.3 What Can Be Reused

The following parts are conceptually reusable and should be preserved where possible:

- request normalization logic for OpenAI and Anthropic payloads
- stream-shaping logic and SSE response handling
- basic routing concepts such as pool membership and round-robin selection
- execution statistics and latency measurement patterns
- existing endpoint and model mapping concepts

## 3. Migration Strategy

The migration should follow one rule:

**Extract stable execution primitives first, then move product concerns outward.**

This means the work should proceed in phases that reduce coupling without requiring a flag-day rewrite.

## 4. Target End State

At the end of this plan, the repository should have the following shape:

```text
unigateway-core/
  src/
    lib.rs
    engine.rs
    pool.rs
    routing.rs
    retry.rs
    request.rs
    response.rs
    hooks.rs
    error.rs
    drivers.rs
    registry.rs
    transport.rs
    protocol/
      mod.rs
      openai.rs
      anthropic.rs

unigateway/
  src/
    main.rs
    server.rs
    gateway/
    cli/
    config/
    mcp.rs
    ...
```

The product-facing crate should depend on `unigateway-core` rather than duplicating execution logic.

## 5. Workstreams

The migration is easiest to manage if tracked as five parallel workstreams.

### 5.1 Workstream A: Core Type System

Goal:

- introduce RFC-aligned request, response, report, error, and hook types

Outputs:

- `PoolId`, `EndpointId`, `DriverId`, `RequestId`
- `ProviderPool`, `Endpoint`, `ExecutionPlan`
- `ProxyChatRequest`, `ProxyResponsesRequest`, `ProxyEmbeddingsRequest`
- `ProxySession`, `CompletedResponse`, `StreamingResponse`
- `RequestReport`, `AttemptReport`, `TokenUsage`
- `GatewayError`

### 5.2 Workstream B: Core Engine and Snapshot State

Goal:

- introduce a pure in-memory engine with snapshot semantics

Outputs:

- `UniGatewayEngine`
- pool upsert and remove APIs
- snapshot capture at request start
- routing strategy selection
- retry and failover orchestration

### 5.3 Workstream C: Driver Layer

Goal:

- replace `llm-connector` coupling with built-in OpenAI-compatible and Anthropic drivers plus a registry model

Outputs:

- `DriverRegistry`
- `ProviderDriver`
- built-in OpenAI-compatible driver
- built-in Anthropic driver
- transport abstraction owned by the core crate

### 5.4 Workstream D: HTTP Adapter Refactor

Goal:

- reduce the current HTTP layer to translation and forwarding only

Outputs:

- HTTP payload parsing into core request types
- HTTP stream forwarding from `ProxySession`
- translation between HTTP auth result and `ExecutionTarget`
- removal of execution policy from handlers

### 5.5 Workstream E: Product-Layer Extraction

Goal:

- push persistence, admin mutations, auth, quota, and operational tooling out of the core runtime path

Outputs:

- config persistence remains in product layer only
- admin APIs remain in product layer only
- gateway API key logic remains in product layer only
- CLI and MCP remain in product layer only

## 6. Phase Plan

## Phase 0: Lock Scope and Naming

Objective:

- freeze the architecture direction before touching implementation

Tasks:

- approve the RFC and implementation plan as the working source of truth
- confirm package naming strategy for `unigateway-core`
- confirm package naming strategy for `unigateway-runtime`
- decide whether the new core lives in the same workspace first or in a new sibling crate immediately
- confirm whether the initial migration keeps the current binary crate name unchanged

Exit Criteria:

- no remaining disagreement on core boundary
- no ambiguity on whether `llm-connector` remains in core
- no ambiguity on execution scope for v1

Status: complete on 2026-04-06.

## Phase 1: Introduce Core-Native Types

Objective:

- create the type system that future code will target

Tasks:

- create core-owned request types
- create core-owned response and report types
- create core-owned error types
- create core-owned pool and endpoint types
- create hook and driver traits

Notes:

- this phase should not yet move the HTTP layer
- this phase should not yet remove existing config structures
- adapters can temporarily convert between old and new types

Exit Criteria:

- core API types compile independently of product-specific auth and persistence
- no public core type references `llm-connector`
- no public core type references Axum types

Status: complete on 2026-04-06.

## Phase 2: Extract Pure In-Memory Engine State

Objective:

- separate runtime execution state from config-file persistence and admin mutation logic

Tasks:

- introduce `UniGatewayEngine` as a standalone runtime state owner
- implement atomic `upsert_pool` and `remove_pool`
- implement request-time snapshot capture
- implement v1 routing strategies: `Random` and `RoundRobin`
- implement retry and failover policy application on top of snapshots

Current Source Areas Affected:

- `src/config.rs`
- `src/config/select.rs`
- `src/config/admin.rs`
- `src/routing.rs`

Migration Rule:

- runtime selection logic moves into core
- config-file write and admin mutation helpers stay outside core

Exit Criteria:

- routing can run without `AppConfig`
- routing can run without `persist_if_dirty`
- execution candidate resolution no longer depends on service-bound gateway API keys

## Phase 3: Replace `llm-connector` in the Execution Path

Objective:

- own the provider execution layer directly inside the core crate

Status: in progress on 2026-04-06.

Implemented so far:

- core-owned `HttpTransport` abstraction and `ReqwestHttpTransport`
- initial built-in `OpenAiCompatibleDriver`
- initial built-in `AnthropicDriver`
- protocol request builders and non-streaming response parsing for the first supported paths
- initial streaming support for OpenAI-compatible chat and responses
- initial streaming support for Anthropic chat

Still pending in this phase:

- retry boundary integration across real upstream failures
- remaining streaming coverage and HTTP adapter wiring for product handlers
- full handler-path migration off `llm-connector`

Tasks:

- define transport abstraction owned by the core crate
- implement built-in OpenAI-compatible driver
- implement built-in Anthropic driver
- port existing request normalization and response shaping logic away from `llm-connector`
- preserve stream semantics and usage extraction where possible

Current Source Areas Affected:

- `src/protocol.rs`
- `src/protocol/client.rs`
- `src/protocol/messages.rs`
- `src/gateway/chat.rs`
- `src/gateway/streaming.rs`
- `src/gateway.rs`

Key Risk:

- stream behavior regression during transport replacement

Mitigation:

- keep stream tests and golden examples for OpenAI and Anthropic wire output
- migrate one protocol path at a time

Exit Criteria:

- core chat, responses, and embeddings paths no longer depend on `llm-connector`
- built-in drivers support the same minimum scenarios covered today
- stream retry boundary remains compliant with the RFC

## Phase 4: Move Orchestration Out of HTTP Handlers

Objective:

- make the HTTP layer a thin adapter over the engine

Tasks:

- parse HTTP payloads into core request types
- translate auth result into pool selection or execution plan
- call `unigateway-core` from handlers
- forward stream and completion outputs to downstream clients
- move request reporting and hook execution into the engine path

Current Source Areas Affected:

- `src/server.rs`
- `src/gateway.rs`
- `src/gateway/chat.rs`
- `src/gateway/streaming.rs`
- `src/system.rs`

Migration Rule:

- handlers should stop making routing and retry decisions directly
- handlers should stop iterating provider lists directly

Exit Criteria:

- `server.rs` only wires routes and application state
- HTTP handlers only parse, delegate, and format
- fallback orchestration is no longer duplicated across handlers

## Phase 5: Push Product Concerns Outward

Objective:

- ensure auth, quota, persistence, admin APIs, and operational tooling are outside the core runtime model

Tasks:

- keep gateway auth and quota logic in product middleware only
- keep config persistence in product config modules only
- keep admin CRUD endpoints in product layer only
- adapt CLI, guide, and MCP operations to talk to product-owned state and then sync pools into core

Current Source Areas Affected:

- `src/middleware.rs`
- `src/authz.rs`
- `src/api_key.rs`
- `src/provider.rs`
- `src/service.rs`
- `src/mcp.rs`
- `src/cli/*`

Exit Criteria:

- product auth and quota code no longer leaks into core types
- admin writes no longer mutate the same structures that define core public APIs
- CLI and MCP use product-facing abstractions layered over core

## Phase 6: Stabilization and Public Crate Preparation

Objective:

- prepare the extracted core for external consumption

Tasks:

- review public API names and docs
- redact or omit secret values on read paths
- finalize hook failure behavior
- finalize embeddings failover behavior
- add crate-level documentation and examples
- add migration notes for embedders such as OpenHub

Exit Criteria:

- public API surface matches the RFC
- internal product modules depend on the core instead of bypassing it
- the crate is publishable without product baggage

## 7. Recommended Execution Order

The recommended order is intentionally conservative.

1. Phase 0
2. Phase 1
3. Phase 2
4. Phase 3
5. Phase 4
6. Phase 5
7. Phase 6

This order reduces risk because it introduces stable types and runtime boundaries before replacing transport and handler orchestration.

## 8. Module Mapping

This table maps current modules to their likely destination.

| Current module | Likely destination | Notes |
| --- | --- | --- |
| `src/routing.rs` | `unigateway-core/src/routing.rs` | Keep routing concepts, rewrite around pools and snapshots |
| `src/protocol.rs` | split between `protocol/*` and `request.rs` | Remove `llm-connector` dependency |
| `src/protocol/client.rs` | `drivers.rs` and `transport.rs` | Replace connector calls with owned transport |
| `src/gateway/chat.rs` | partly core, partly HTTP adapter | execution leaves handler layer |
| `src/gateway/streaming.rs` | partly core, partly HTTP adapter | stream wiring split from execution |
| `src/gateway.rs` | thin product adapter | orchestration should move into engine |
| `src/config/select.rs` | core engine selection logic | keep only runtime selection concepts |
| `src/config/admin.rs` | product layer only | persistence and admin writes stay out of core |
| `src/config/store.rs` | product layer only | core must not persist config |
| `src/middleware.rs` | product layer only | auth, quota, and rate limits remain outside core |
| `src/server.rs` | product layer only | route registration only |
| `src/api_key.rs` | product layer only | not part of core |
| `src/provider.rs` | product layer only | admin CRUD, not core |
| `src/service.rs` | product layer only | admin CRUD, not core |
| `src/mcp.rs` | product layer only | operational tools, not core |

## 9. Testing Plan

The migration should be validated by capability, not just by unit coverage.

### 9.1 Core Engine Tests

- pool snapshot replacement behavior
- random and round-robin endpoint selection
- retry behavior on `429`
- retry behavior on `5xx`
- retry behavior on timeout and transport failure
- stop retry after first downstream stream event

### 9.2 Driver Tests

- OpenAI-compatible non-streaming chat
- OpenAI-compatible streaming chat
- Anthropic non-streaming messages
- Anthropic streaming messages
- responses API support
- embeddings API support

### 9.3 Product-Layer Integration Tests

- auth succeeds and selects correct target pool
- product layer converts HTTP requests to core requests correctly
- admin/config updates propagate into core pool snapshots
- CLI and MCP operations continue to work after extraction

## 10. Major Risks and Mitigations

### Risk 1: Stream Compatibility Regression

Risk:

- replacing `llm-connector` may break exact stream behavior

Mitigation:

- capture stream fixtures before migration
- add protocol-level regression tests before replacing transport
- migrate OpenAI-compatible and Anthropic streams separately

### Risk 2: Half-Extracted State Model

Risk:

- runtime state and persisted state may remain partially intertwined

Mitigation:

- explicitly prohibit config persistence code from entering core modules
- keep a separate product-owned synchronization layer between persisted config and runtime engine state

### Risk 3: HTTP Layer Still Owning Execution Logic

Risk:

- handlers may continue to accumulate fallback and retry behavior even after core exists

Mitigation:

- require that fallback loops live only in the engine
- require that handlers only parse input and forward output

### Risk 4: Over-Designing Plugin APIs Too Early

Risk:

- a public plugin boundary may become too broad before real extension cases are tested

Mitigation:

- keep v1 plugin surface minimal
- support custom drivers without promising a full ecosystem framework on day one

## 11. Milestone Definition

The migration should be considered successful when all of the following are true:

1. a reusable `unigateway-core` crate exists with no database and no `llm-connector` dependency
2. the product-facing crate delegates request execution to the core instead of owning fallback loops itself
3. OpenAI-compatible and Anthropic built-in drivers cover the current primary use cases
4. admin/config/product concerns are layered above the core instead of embedded within it
5. OpenHub-style embedding is possible by pushing pool snapshots into the engine directly

## 12. Immediate Next Steps

The recommended immediate next development actions are:

1. create the new crate or internal module boundary for `unigateway-core`
2. implement Phase 1 core-native type definitions first
3. extract pure in-memory pool state before touching transport replacement
4. only then begin replacing `llm-connector` with built-in drivers

This keeps the early iterations focused on shape and boundaries rather than transport churn.
