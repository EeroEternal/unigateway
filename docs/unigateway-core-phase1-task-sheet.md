# UniGateway Core Phase 1 Task Sheet

Status: Complete

Date: 2026-04-06

Completed on: 2026-04-06

Related documents:

- [docs/unigateway-core-api-draft.md](docs/unigateway-core-api-draft.md)
- [docs/unigateway-core-implementation-plan.md](docs/unigateway-core-implementation-plan.md)
- [docs/unigateway-core-phase0-checklist.md](docs/unigateway-core-phase0-checklist.md)

## 1. Purpose

This document defines the detailed work for Phase 1: introducing core-native types and the initial `unigateway-core` module skeleton.

Phase 1 is intentionally narrow. It should establish stable type boundaries before any transport replacement or handler refactor begins.

## 2. Phase 1 Objective

Create the first implementation slice of `unigateway-core` by defining:

- core-owned request types
- core-owned response and report types
- core-owned pool and endpoint types
- core-owned error types
- core-owned hook and driver traits
- the initial crate/module skeleton

Phase 1 does not include transport replacement, Axum refactor, or persistence changes.

## 2.1 Recommended Phase 1 Execution Strategy

Phase 1 should be executed as a boundary-establishment phase, not as a behavior-change phase.

The correct mindset is:

- introduce the new crate first
- make it compile with minimal public exports
- add the core-owned type system
- avoid moving runtime behavior too early
- avoid touching current request execution until the new type boundary is stable

This phase should end with a new crate and a stable public type surface, but with almost no user-visible behavior changes.

## 3. Deliverables

Phase 1 should produce the following concrete outputs.

### 3.1 Crate Boundary

- introduce the `unigateway-core` crate in the current repository
- add a minimal `lib.rs`
- ensure the product-facing crate can depend on it locally

### 3.2 Core Type Modules

- `request.rs`
- `response.rs`
- `pool.rs`
- `error.rs`
- `hooks.rs`
- `drivers.rs`

### 3.3 Initial Public Types

- `PoolId`, `EndpointId`, `DriverId`, `RequestId`
- `ProviderPool`, `Endpoint`, `ExecutionPlan`, `EndpointRef`
- `ProxyChatRequest`, `ProxyResponsesRequest`, `ProxyEmbeddingsRequest`
- `Message`, `MessageRole`
- `ProxySession`, `CompletedResponse`, `StreamingResponse`
- `RequestReport`, `AttemptReport`, `AttemptStatus`, `TokenUsage`
- `GatewayError`
- `GatewayHooks`, `DriverRegistry`, `ProviderDriver`, `DriverEndpointContext`

### 3.4 Initial Placeholder Modules

- `engine.rs`
- `routing.rs`
- `retry.rs`
- `registry.rs`
- `transport.rs`
- `protocol/mod.rs`
- `protocol/openai.rs`
- `protocol/anthropic.rs`

The placeholder modules do not need full implementation in Phase 1, but the crate structure should make the future direction obvious.

## 3.5 Phase 1 Success Shape

By the end of Phase 1, the repository should be in this state:

1. the root product crate still builds normally
2. a new `unigateway-core` crate exists in the same repository
3. the new crate exports RFC-aligned public types
4. no execution path has been migrated yet
5. the repository is ready for Phase 2 engine extraction without further type redesign

## 4. Non-Goals for Phase 1

Phase 1 must not include:

- replacing `llm-connector`
- moving fallback loops out of handlers
- implementing runtime engine behavior in full
- moving admin APIs
- changing CLI flows
- changing MCP flows
- changing config persistence logic

## 5. Task Breakdown

## 5.0 Recommended End-to-End Order

The safest execution order for Phase 1 is:

1. introduce the workspace/crate boundary with the least possible Cargo disruption
2. create the new `unigateway-core` crate with only `lib.rs` and placeholder modules
3. make the current product crate depend on `unigateway-core` by local path, even if it is not used yet
4. add the core-owned identifier, pool, request, response, error, hook, and driver types
5. add basic compile-time tests or module-level checks for public type consistency
6. document temporary adapter boundaries from current product types into the new core types

This order minimizes risk because Cargo and module layout are stabilized before any public type surface is expanded.

## Task Group A: Create the Crate Skeleton

Tasks:

1. create `unigateway-core` as a new local crate
2. add `lib.rs`
3. declare the initial module tree
4. wire the crate into the workspace build

Acceptance:

- the new crate compiles with placeholder exports
- no product modules have been migrated yet

Implementation notes:

- prefer keeping the current repository root as the product crate during this phase
- introduce `unigateway-core/` as a sibling crate under the repository root
- avoid renaming the existing package or moving `src/` in Phase 1
- the root `Cargo.toml` should be adjusted in the least disruptive way possible

## Task Group B: Define Identifier and Pool Types

Tasks:

1. define core identifier aliases
2. define `ProviderPool`
3. define `Endpoint`
4. define `ProviderKind`
5. define `ModelPolicy`
6. define `ExecutionPlan` and `EndpointRef`

Acceptance:

- all pool and endpoint concepts are represented in core-owned types
- `Endpoint` remains pure data and does not store behavior objects

Implementation notes:

- these types should be defined before any adapter logic is written
- avoid prematurely adding helper methods that encode product behavior
- keep fields aligned with the RFC terminology, not the current product terminology

## Task Group C: Define Request Types

Tasks:

1. define `ProxyChatRequest`
2. define `ProxyResponsesRequest`
3. define `ProxyEmbeddingsRequest`
4. define `Message` and `MessageRole`

Acceptance:

- no request type depends on Axum
- no request type depends on `llm-connector`

Implementation notes:

- request types should be transport-neutral and protocol-neutral
- do not copy current Axum handler assumptions into the core request model

## Task Group D: Define Response and Report Types

Tasks:

1. define `ProxySession`
2. define `CompletedResponse`
3. define `StreamingResponse`
4. define `ResponseStream` and `CompletionHandle`
5. define `RequestReport`
6. define `AttemptReport`, `AttemptStatus`, and `TokenUsage`

Acceptance:

- stream return shape matches RFC dual-return semantics
- result reporting uses `selected_endpoint_id`

Implementation notes:

- do not try to fully wire stream execution in Phase 1
- only define the return model and the types it depends on

## Task Group E: Define Error, Hook, and Driver Traits

Tasks:

1. define `GatewayError`
2. define `GatewayHooks`
3. define `AttemptStartedEvent`
4. define `AttemptFinishedEvent`
5. define `DriverRegistry`
6. define `ProviderDriver`
7. define `DriverEndpointContext`

Acceptance:

- no trait exposes `reqwest::Request` or `reqwest::Response`
- hook traits are asynchronous
- plugin boundary is defined but minimal

Implementation notes:

- keep the plugin surface deliberately small
- avoid adding convenience traits or provider-specific subtraits until real usage appears

## Task Group F: Define Initial Adapter Boundaries

Tasks:

1. document how current HTTP payload parsing maps into new core request types
2. document how current routing/config structures map into `ProviderPool` and `Endpoint`
3. document which existing modules will temporarily construct these new core types

Acceptance:

- there is a clear transition path from existing structures to core-native types
- Phase 2 can begin without redesigning the type system

Implementation notes:

- adapter documentation is part of the deliverable even if the adapters themselves are not fully implemented yet
- this is where current `config/*`, `routing.rs`, and `protocol.rs` concepts should be mapped into future core types

## 6. Suggested File Layout for Phase 1

```text
unigateway-core/
  Cargo.toml
  src/
    lib.rs
    request.rs
    response.rs
    pool.rs
    error.rs
    hooks.rs
    drivers.rs
    engine.rs
    routing.rs
    retry.rs
    registry.rs
    transport.rs
    protocol/
      mod.rs
      openai.rs
      anthropic.rs
```

## 6.1 Recommended Cargo Strategy

The safest Cargo strategy for Phase 1 is:

1. keep the existing root package as `unigateway`
2. add a workspace definition only if needed to include `unigateway-core`
3. create `unigateway-core/Cargo.toml` as a new sibling crate
4. add `unigateway-core` as a path dependency from the root package
5. avoid moving existing `src/` files during this phase

This keeps build disruption low while establishing the future crate boundary.

### Preferred Outcome for Phase 1

- existing `cargo build` for the product crate still works
- `cargo check -p unigateway-core` works independently
- the product crate does not yet need to consume the new core types in runtime code

## 7. Dependency Rules for Phase 1

The following rules should be enforced in Phase 1.

### Allowed in `unigateway-core`

- standard library
- async primitives
- serde and serde_json if needed for request payload types
- minimal utility crates required by the public type system

### Not Allowed in `unigateway-core`

- Axum
- Clap
- MCP-related crates
- config persistence logic
- admin API handlers
- `llm-connector`
- database libraries

### Allowed but Should Be Minimized

- `tokio`, only if needed for public async type aliases or tests
- `futures-core` or `futures-util`, only where required by stream and hook type definitions
- secret-handling crate(s), only if the public endpoint model needs a stable secret wrapper

## 8. Code Review Checklist for Phase 1

Every Phase 1 review should verify:

1. no product-layer concern leaked into the new core types
2. no core public type references `llm-connector`
3. no core public trait exposes `reqwest` request or response objects
4. no secret values are required in read/report APIs beyond endpoint-owned execution context
5. type names and module names match the RFC language
6. the new crate boundary does not break the existing root package build
7. the new crate does not accidentally import product modules from the root crate

## 9. Exit Criteria

Phase 1 is complete when all of the following are true:

1. `unigateway-core` exists as a local crate or equivalent isolated module boundary.
2. RFC-owned public types are defined in core-owned modules.
3. The type layer compiles without Axum or `llm-connector` in the public surface.
4. The crate structure is ready for Phase 2 engine extraction.
5. There is no unresolved ambiguity about the core public type system.

## 9.1 Validation Gates During Phase 1

The following checkpoints should be treated as mandatory gates.

### Gate A: After Crate Creation

- the repository still builds
- the new crate builds
- no runtime code has been moved yet

### Gate B: After Public Type Definitions

- all RFC-owned public types compile
- no forbidden dependencies have entered `unigateway-core`
- no product-layer imports exist inside the new crate

### Gate C: Before Declaring Phase 1 Complete

- the module skeleton matches the RFC direction
- the current root crate remains behaviorally unchanged
- the Phase 2 engine extraction can start without revisiting type naming

## 9.2 Explicit Do-Not-Touch Boundaries for Phase 1

To avoid scope drift, Phase 1 should not modify the following runtime behaviors:

- current HTTP route behavior
- current auth and quota enforcement behavior
- current config persistence behavior
- current admin API behavior
- current CLI and MCP behavior
- current fallback execution behavior

If a change proposal touches those areas, it belongs to a later phase.

## 10. Recommended Next Step After Phase 1

Once Phase 1 completes, the next planned step is Phase 2:

- extract pure in-memory engine state
- move routing selection into the core runtime
- separate runtime state from persistence and admin mutation helpers

## 11. Initial Adapter Mapping

This section records the intended mapping from current product-owned structures into the new core-native type system.

### 11.1 Config and Routing Mapping

Current source structures:

- `GatewayConfigFile.services`
- `GatewayConfigFile.providers`
- `GatewayConfigFile.bindings`
- `ServiceProvider`
- `ResolvedProvider`

Target mapping:

- one current `service` maps to one future `ProviderPool`
- the current binding list becomes the source of endpoint membership in that pool
- each current bound provider becomes one future `Endpoint`
- current `provider_type` maps to `ProviderKind`
- current provider `name` or normalized identity becomes the future `endpoint_id`
- current `default_model` and `model_mapping` map into `ModelPolicy`
- current base URL resolution logic informs later driver execution, but the Phase 1 type layer only stores normalized endpoint fields

### 11.2 Request Mapping

Current source functions:

- `openai_payload_to_chat_request`
- `anthropic_payload_to_chat_request`
- `openai_payload_to_responses_request`
- `openai_payload_to_embed_request`

Target mapping:

- OpenAI chat payloads map to `ProxyChatRequest`
- Anthropic messages payloads also map to `ProxyChatRequest`
- OpenAI responses payloads map to `ProxyResponsesRequest`
- OpenAI embeddings payloads map to `ProxyEmbeddingsRequest`

Phase 1 rule:

- keep the current parsing functions where they are for now
- later phases should switch them from producing `llm-connector` request types to producing core-native request types

### 11.3 Response Mapping

Current source functions:

- `chat_response_to_openai_json`
- `chat_response_to_anthropic_json`
- `embed_response_to_openai_json`
- SSE shaping in `gateway/streaming.rs`

Target mapping:

- transport and provider execution will eventually produce `ChatResponseChunk`, `ChatResponseFinal`, `ResponsesEvent`, `ResponsesFinal`, and `EmbeddingsResponse`
- HTTP adapters in the product layer will remain responsible for final wire-format shaping toward clients

Phase 1 rule:

- only the core response types are introduced now
- no current response shaping logic is moved yet

### 11.4 Auth and Product State Boundary

Current product-owned structures that must remain outside core:

- `AppConfig`
- `AppState`
- `GatewayAuth`
- API key quota and runtime rate-limit state
- config persistence and admin mutation helpers

Phase 1 rule:

- none of these structures should appear in `unigateway-core`
- later adapter code in the product crate will translate auth results into `ExecutionTarget`

### 11.5 Driver Boundary

Current source area:

- `protocol/client.rs`
- `gateway/chat.rs`
- `gateway/streaming.rs`

Target mapping:

- current connector-owned transport execution will be replaced in later phases by core-owned built-in drivers and transport primitives
- Phase 1 only introduces the trait surface: `DriverRegistry`, `ProviderDriver`, and `DriverEndpointContext`

This mapping is sufficient for Phase 2 to begin without revisiting the public type system.
