# OpenHub Gateway Migration Plan

Status: Draft

Date: 2026-04-07

Scope: OpenHub Gateway migration onto `unigateway-core` and `unigateway-runtime`

## 1. Purpose

This document defines a practical migration plan for moving OpenHub Gateway onto the `unigateway-core` and `unigateway-runtime` stack without losing OpenHub's product semantics.

The goal is not to restore the old gateway engine. The goal is to keep the new thin-shell architecture and progressively rebuild the business layers that a real gateway still needs:

- protocol translation
- authentication and authorization
- model and provider routing
- in-memory pool snapshots
- usage tracking and compatibility endpoints

This plan assumes the current rewrite has already proven one important point:

**OpenHub can execute requests through `unigateway-core`, but the current prototype is too thin to be production-credible.**

## 2. Current Assessment

The current OpenHub rewrite is best understood as a technical spike, not a merge-ready gateway.

### 2.1 What the rewrite got right

- It removed duplicated execution logic that overlaps with `unigateway-core`.
- It proved that OpenHub can host `UniGatewayEngine` and built-in drivers.
- It moved the system toward a cleaner layered structure: HTTP shell -> runtime host -> core engine.

### 2.2 What the rewrite broke

- It exposed core-native request structs directly on HTTP routes.
- It replaced real routing semantics with a hard-coded `default-service`.
- It assumed `RuntimePoolHost` provided caching semantics that it does not provide.
- It mapped unsupported concepts such as endpoint weights into metadata that the core engine does not consume.
- It removed OpenHub-specific control-plane behavior without replacing it.
- It reduced compatibility surface too aggressively.

### 2.3 Correct framing

The problem is not that the new architecture is wrong.

The problem is that OpenHub-specific gateway concerns were deleted faster than they were reattached to the new architecture.

The migration target should therefore be:

**thin but complete**, not **thin but empty**.

## 3. Migration Principles

The migration should follow five rules.

### 3.1 Keep the new engine stack

Do not restore the old OpenHub execution engine, adapter stack, or pool manager.

`unigateway-core` should remain the only execution engine.

### 3.2 Restore product semantics outside the engine

OpenHub-specific concerns must live in the gateway shell and host layer:

- request sanitization
- auth and key lookup
- pricing and model permission checks
- provider-account selection
- reporting and billing hooks

### 3.3 Move database reads out of the hot path

The request path should not build pools by querying PostgreSQL on every request.

Database state should be synchronized into in-memory `ProviderPool` snapshots and pushed into `UniGatewayEngine` via `upsert_pool`.

### 3.4 Do not invent core capabilities that do not exist

If OpenHub requires weighted routing, cache invalidation semantics, or richer scheduling, that requirement must be implemented explicitly.

Do not treat metadata fields as if they change engine behavior.

### 3.5 Preserve external compatibility where it matters

OpenHub is still a gateway product. It must keep tolerant HTTP behavior and the compatibility endpoints that real clients depend on.

## 4. Target End State

At the end of this migration, the OpenHub request path should look like this:

```text
HTTP Request
  -> permissive HTTP payload parser
  -> OpenHub auth and policy middleware
  -> model / pricing / provider-account resolution
  -> execution target selection
  -> UniGatewayEngine proxy_chat / proxy_responses / proxy_embeddings
  -> response adaptation to OpenAI / Anthropic / emulator surface
  -> async usage tracking and activity persistence
```

The key distinction is:

- `unigateway-core` owns execution
- OpenHub owns control-plane semantics
- synchronization bridges database state into core pools

## 5. Required Closures

The migration should be organized around five closures that must all be completed before the rewrite can be considered merge-ready.

### 5.1 Closure A: Translator Layer

OpenHub must not deserialize external HTTP payloads directly into `ProxyChatRequest`, `ProxyResponsesRequest`, or `ProxyEmbeddingsRequest`.

Instead, it should introduce permissive request models that can absorb real client variance:

- optional fields instead of hard-required internal fields
- tolerant role parsing
- tolerant tool and stream parsing
- preservation or stripping of unknown fields according to route contract

Outputs:

- `translators/openai.rs`
- `translators/anthropic.rs` if Anthropic surface is restored
- HTTP payload -> core request conversion helpers
- clear validation errors for unsupported payload shapes

Acceptance criteria:

- standard OpenAI-compatible clients can call chat, responses, and embeddings without sending core-native fields
- invalid payloads fail at the translator boundary, not deep in the engine

### 5.2 Closure B: Auth and Routing Lifecycle

OpenHub must restore its real control-plane flow before a request reaches the engine.

The expected lifecycle is:

1. extract bearer or gateway token
2. validate token against OpenHub user-facing key tables
3. determine the requested model and protocol
4. resolve pricing and permission constraints
5. map request to a concrete provider-account pool or execution plan

This stage should produce a stable internal routing object such as:

- authenticated principal
- requested model
- resolved pool id
- optional endpoint hint
- usage attribution context

Outputs:

- request auth middleware or pre-handler context builder
- user key lookup and validation
- model access and pricing resolution
- deterministic mapping from OpenHub business state to `PoolId` / `ExecutionTarget`

Acceptance criteria:

- hard-coded service ids are fully removed
- unauthorized or disallowed model access is rejected before engine invocation
- provider selection is explainable from database state

### 5.3 Closure C: Snapshot Sync

This is the most important structural correction.

The request path should stop querying database tables to construct a pool for every request.

Instead:

- on startup, load the relevant provider-account and key state from PostgreSQL
- build `ProviderPool` snapshots
- push them into `UniGatewayEngine` using `upsert_pool`
- on database change, rebuild only affected pools and replace them in memory

Recommended mechanism:

- initial full sync on startup
- background refresh task
- optional PostgreSQL `LISTEN/NOTIFY` if operationally justified

Outputs:

- snapshot builder from OpenHub tables to `ProviderPool`
- sync coordinator
- pool invalidation and replacement logic
- clear mapping rules for account status and key status

Acceptance criteria:

- the hot request path performs no pool-building SQL
- a pool update becomes visible through in-memory engine state without process restart
- removed or disabled provider accounts are removed from engine state explicitly

### 5.4 Closure D: Usage Tracking and Hooks

Once the engine path is stable, OpenHub should attach a hook implementation to capture execution reports.

Hook responsibilities may include:

- request activity logs
- token usage accounting
- latency and failure tracking
- downstream billing writes or queued aggregation

This closure must remain post-execution only. It should not be used to replace admission checks that belong before engine invocation.

Outputs:

- `GatewayHooks` implementation for OpenHub
- async persistence or queueing strategy for request reports
- mapping from `RequestReport` and `TokenUsage` into OpenHub activity tables

Acceptance criteria:

- successful and failed requests produce durable activity records
- usage accounting is sourced from engine reports instead of duplicated ad hoc parsing

### 5.5 Closure E: Compatibility Surface Recovery

The rewrite should restore the compatibility endpoints that matter to OpenHub users.

Minimum recovery set:

- `POST /v1/chat/completions`
- `POST /v1/embeddings`
- `GET /v1/models`

Optional recovery set, depending on real client demand:

- `POST /v1/messages`
- Ollama-compatible endpoints
- additional OpenAI emulator paths used by existing integrations

Acceptance criteria:

- the gateway exposes the minimal compatibility surface required by current OpenHub clients
- model listing reflects real OpenHub-visible models, not placeholder defaults

## 6. Three-Phase Execution Plan

The five closures should be delivered in three implementation phases.

## Phase 1: Make the Request Path Honest

Goal:

- eliminate the prototype shortcuts that make the current gateway misleading

Work:

- add permissive HTTP request structs and translators
- remove direct `Json<Proxy*>` public route handling
- remove hard-coded `default-service`
- reintroduce real auth and route-resolution context
- align runtime SQL and data-model assumptions with the real schema

Non-goals:

- no weighted routing
- no full compatibility surface recovery yet
- no background sync yet if startup sync is sufficient for first cut

Exit criteria:

- a real OpenAI client can send a request through the gateway
- the request is authenticated and mapped using OpenHub business state
- no runtime SQL references nonexistent columns or fields

## Phase 2: Move State Out of the Hot Path

Goal:

- make the system operationally sane under load

Work:

- build startup snapshot sync from database -> `ProviderPool`
- push pools into `UniGatewayEngine`
- replace per-request pool SQL with engine-backed in-memory pools
- add incremental refresh or explicit resync hooks
- define removal behavior for disabled accounts and keys

Non-goals:

- no advanced scheduling features beyond current core capabilities

Exit criteria:

- request handling does not construct pools from database reads
- sync and refresh behavior is testable and deterministic
- engine state reflects OpenHub provider-account status changes

## Phase 3: Restore Product Completeness

Goal:

- bring the thin-shell gateway back to product-grade completeness

Work:

- attach usage-tracking hooks
- restore `/v1/models`
- restore additional compatibility endpoints as needed
- add operational logging and tracing around sync, auth, and execution
- document the new control-plane to data-plane flow

Exit criteria:

- the new gateway can replace the old one for intended client traffic
- compatibility regressions are explicitly understood and documented
- request execution, usage reporting, and model discovery all function end-to-end

## 7. Recommended Module Shape

The OpenHub gateway should converge toward a structure like this:

```text
gateway/src/
  api/
    openai.rs
    anthropic.rs
    models.rs
  auth/
    middleware.rs
    keys.rs
  translators/
    openai.rs
    anthropic.rs
  routing/
    resolve.rs
    context.rs
  sync/
    bootstrap.rs
    notify.rs
    pools.rs
  usage/
    hooks.rs
    activity.rs
  runtime.rs
  main.rs
```

Important rule:

Do not recreate the old adapter, engine, or pool subsystems under new names.

## 8. What Not To Do

The migration should explicitly avoid the following mistakes.

### 8.1 Do not expose core-native structs as public HTTP contracts

Those types are internal execution contracts, not compatibility-layer request schemas.

### 8.2 Do not query PostgreSQL to build pools on every request

That erases the benefit of the in-memory engine and creates avoidable load and latency.

### 8.3 Do not encode unsupported scheduling semantics in metadata

If weighted routing is required, it must either be implemented in core or modeled explicitly elsewhere.

### 8.4 Do not recover the old gateway by restoring the old engine

That would reverse the architectural gain of this migration.

### 8.5 Do not block migration on perfect endpoint parity

Recover the minimum client-critical surface first. Expand only after the main path is correct.

## 9. Merge Readiness Checklist

The rewrite should not be considered merge-ready until the following statements are true.

- HTTP handlers accept external protocol payloads, not core-native structs.
- Authentication and model access checks are restored.
- Real database schema is used without guessed columns.
- Pool state is synchronized into engine memory instead of read per request.
- Request activity and token usage are persisted from engine reports.
- `/v1/models` is restored and reflects real OpenHub-visible data.
- Current client-critical compatibility routes are available.
- The codebase no longer contains placeholder routing identifiers such as `default-service`.

## 10. Final Recommendation

The right migration strategy is:

**keep the new clean shell, restore the missing business layers, and do not resurrect the old execution stack.**

OpenHub should move from:

- heavy but complete

to:

- thin but empty

and finally to:

- thin and complete

That is the correct path to a mergeable OpenHub gateway on top of `unigateway-core`.