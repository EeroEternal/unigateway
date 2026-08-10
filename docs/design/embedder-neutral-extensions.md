---
title: Embedder-Neutral Gateway Extensions
---

# Embedder-Neutral Gateway Extensions

This document describes a planned set of **opt-in, embedder-neutral** improvements to UniGateway. The work fixes a real protocol-layer defect (gateway-internal JSON fields leaking to upstream providers), adds reusable host extension points, and improves embedder documentation—without baking any single consumer product (for example Zene) into core.

Draft GitHub issue bodies live in [`dev/issues/`](../dev/issues/README.md).

## Background

Embedders often run UniGateway as a library inside their own HTTP gateway. Clients send OpenAI- or Anthropic-shaped JSON; the embedder parses, optionally assembles or mutates the request, forwards through `UniGatewayEngine`, and renders the response.

A common pattern is to attach **gateway-internal metadata** in the request body (session delivery hints, trace context, prefix version tokens). Keys prefixed with `_` are a widely understood convention for “private / local extensions” that must not be interpreted by external APIs.

Today, those fields are treated like vendor extensions and forwarded upstream. That is incorrect for OpenAI-compatible providers and forces embedders to re-parse raw JSON or maintain parallel side channels.

## Design principles

1. **Core MUST NOT contain product-specific logic** — no hardcoded consumer names, routes, or agent semantics in `unigateway-core`.
2. **Default behavior unchanged** — features off or unset means the same behavior as current 2.x for normal requests.
3. **Convention over vendor lists** — a single `_` prefix rule instead of maintaining allow/deny lists per embedder.
4. **State and proxy separated** — session stores, HTTP admin routes, and delta assembly policy live in host/middleware or embedder code, not in `UniGatewayEngine` global state.
5. **Opt-in extension** — middleware, session reference crates, and metadata→header forwarding are explicit configuration or features.

These align with [`AGENTS.md`](../../AGENTS.md) and [`arch.md`](arch.md).

## Problem (current behavior)

### Parse: unknown fields → `extra`

In `unigateway-protocol/src/requests.rs`, `openai_chat_extra` and `anthropic_chat_extra` collect every top-level key that is not a known standard field into `ProxyChatRequest.extra`.

### Driver: `extra` → upstream body

OpenAI and Anthropic drivers merge all of `extra` into the upstream JSON payload (`unigateway-core/src/protocol/openai/requests.rs`, `anthropic/requests.rs`).

### Result

A client or embedder body such as:

```json
{
  "model": "gpt-4o-mini",
  "messages": [{"role": "user", "content": "hi"}],
  "_session_context": {"epoch": 3, "delivery": "delta"}
}
```

currently produces an upstream HTTP body that **includes** `_session_context`. Upstream APIs may reject unknown keys or behave unpredictably.

### What is not broken

- **Ingress only:** an embedder HTTP server can already *read* `_` fields from the raw JSON body before forwarding.
- **The bug is on the embedder → upstream hop** when parsing puts `_` keys into `extra` and drivers merge them.

## Scope overview

| ID | Item | Priority | Default impact |
| --- | --- | --- | --- |
| R1 | Strip `_`-prefixed keys from upstream merge | High | `_` keys no longer forwarded (documented behavior change) |
| R2 | `gateway_fields` bucket on proxy requests | High | Additive; empty by default |
| R3 | Configurable `metadata` → outbound HTTP headers | Low | None (`None` = no forwarding) |
| R4 | Host middleware hooks (request / response) | Medium | None (empty chain) |
| R5 | Session prefix reference implementation | Optional | None (feature / separate crate) |
| R6 | Production OpenAI passthrough example | Medium | Example only |
| R7 | Prompt cache usage parsing | Docs / minor | Already largely implemented in core |

## Work items (detail)

### R1 — Gateway-only body field convention

**Rule:** top-level keys whose names start with a single underscore (`_`) MUST NOT be forwarded to upstream providers.

**Where:**

- Parse time: exclude from `extra` when collecting unknown fields.
- Driver merge time: defensive skip if any `_` key remains in `extra` (belt and suspenders).

**Non-goals:** define semantics for any specific `_` key; alter known OpenAI / Anthropic standard fields.

### R2 — `gateway_fields` bucket

Add `gateway_fields: HashMap<String, Value>` to `ProxyChatRequest` (and symmetric types if needed).

| Field | Purpose | Upstream |
| --- | --- | --- |
| `extra` | Vendor extensions to forward (`reasoning_effort`, etc.) | Merged by driver |
| `metadata` | Core / host string metadata (`unigateway.*`) | Internal; hooks, routing hints |
| `gateway_fields` | Embedder gateway-only JSON (`_*` at ingress) | **Never** merged |

Middleware reads `gateway_fields` without re-parsing the raw HTTP body.

**Semver:** additive field → minor release. Changelog MUST note that `_`-prefixed top-level fields are no longer upstream-forwarded.

### R3 — Metadata → HTTP header forwarding (optional)

**Static headers:** endpoint `metadata` keys prefixed with `http_header.*` (`openai_headers` in core).

**Per-request headers:** optional `forward_metadata_as_headers: Option<Vec<String>>` on `Endpoint` / `ProviderPool` (explicit allowlist / glob). Default `None` = no forwarding. `unigateway.*` keys are never forwarded.

**Documentation:** [`protocol-conversion.md`](protocol-conversion.md#outbound-http-headers), [`embedder_patterns.md`](../guide/embedder_patterns.md) (模式六).

### R4 — Host middleware hooks

Core `GatewayHooks` observe lifecycle (attempt started/finished, reports). They do not mutate requests.

Host middleware (opt-in chain in `unigateway-host`):

- **Request:** after parse (+ optional session assembly), before `proxy_chat`; can mutate `ProxyChatRequest` and read `gateway_fields`.
- **Response:** after `ProxySession`, before protocol render (optional).

Default chain is empty → no behavior change.

### R5 — Session prefix reference (optional)

Generic primitives for agent products: publish stored prefix, epoch / hash validation, delta assembly (`full = stored_prefix || tail`). Implemented in **`unigateway-session`** (opt-in crate; `http` feature for publish/delete routes).

Embedders with existing session stores can use R1+R2+R4 only. See [`unigateway-session/README.md`](../../unigateway-session/README.md) and [`embedder_patterns.md`](../guide/embedder_patterns.md) (模式七).

### R6 — Production passthrough example

Replace the debug stub in `unigateway-sdk/examples/openai_passthrough.rs` with:

1. Axum `POST /v1/chat/completions`
2. `openai_payload_to_chat_request`
3. Host dispatch or `engine.proxy_chat`
4. `render_openai_chat_session`
5. `ProtocolHttpResponse` → Axum (JSON + SSE)

See [`dev/issues/03-passthrough-example.md`](../dev/issues/03-passthrough-example.md).

### R7 — Usage / prompt cache fields

OpenAI-compatible usage parsing maps vendor cache fields to `TokenUsage.cache_hit_tokens` / `cache_write_tokens` (`unigateway-core/src/protocol/openai/parsing.rs`). Documented in [`usage-cache.md`](../guide/usage-cache.md).

## Request pipeline (normative)

Embedders and host middleware should follow this order:

```text
┌─────────────────────────────────────────────────────────────────┐
│ 1. Ingress (embedder HTTP) — raw JSON body                      │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. Protocol parse (unigateway-protocol)                         │
│    • standard fields → ProxyChatRequest                         │
│    • `_` top-level keys → gateway_fields                        │
│    • other unknown keys → extra (forwardable vendor extensions) │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. Optional session / delta assembly (embedder or R5 reference) │
│    • read gateway_fields                                        │
│    • mutate messages (e.g. prefix || tail)                      │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. Host request middleware (R4, opt-in)                         │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. Host dispatch → UniGatewayEngine::proxy_chat                 │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 6. Driver upstream merge (R1: ignore gateway_fields; strip _) │
└───────────────────────────────┬─────────────────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ 7. Optional host response middleware → protocol render → HTTP   │
└─────────────────────────────────────────────────────────────────┘
```

Middleware that assembles deltas MUST run **after** parse (needs `gateway_fields`) and **before** driver merge (final `messages` shape).

## Embedder topology

| Topology | R1+R2 benefit |
| --- | --- |
| HTTP gateway embedder → upstream | **Primary** — fixes parse + forward leak |
| Client sends `_` fields in JSON to gateway | Ingress read already works; fix is upstream strip |
| In-process `UniGatewayEngine` → upstream | Driver merge also protected |
| Plain HTTP client, no core on client | Unaffected |

Dual-hop setups (client-side engine + gateway-side engine) are an architecture choice, not a blocker for this work.

## Explicit non-goals

- Hardcoded consumer routes (e.g. `/v1/<product>/sessions/...`) in core or default host.
- Compaction, memory flush, or tool-output handle semantics in core.
- Removing support for full-message delivery (`delivery=full` or equivalent).
- Restoring a mandatory in-tree gateway binary; remain embed-first.

## Implementation roadmap

### Phase 1 — Protocol fix (R1 + R2) **← start here**

1. Add `gateway_fields` to `ProxyChatRequest` in `unigateway-core/src/request.rs` (default empty).
2. Split `openai_chat_extra` / `anthropic_chat_extra` in `unigateway-protocol/src/requests.rs`:
   - keys starting with `_` → `gateway_fields`
   - other unknown keys → `extra`
3. Ensure OpenAI / Anthropic drivers never merge `gateway_fields`; skip `_` keys in `extra` merge loops.
4. Update manual `ProxyChatRequest` constructors in tests.
5. Add regression tests: `_foo` absent upstream; `reasoning_effort` still forwarded; middleware can read `gateway_fields`.
6. Update [`protocol-conversion.md`](protocol-conversion.md) preservation channels section.

**Issue:** [`dev/issues/01-gateway-fields-upstream-strip.md`](../dev/issues/01-gateway-fields-upstream-strip.md)

### Phase 2 — Embedder template (R6)

1. Rewrite `openai_passthrough` example to use render + SSE.
2. Document env vars and mock-upstream smoke test in example header comment.
3. Optional: small `ProtocolHttpResponse` → Axum helper in example file.

**Issue:** [`dev/issues/03-passthrough-example.md`](../dev/issues/03-passthrough-example.md)

### Phase 3 — Host extension (R4)

1. Define `ChatRequestMiddleware` / optional response middleware traits in `unigateway-host`.
2. Wire empty default chain into dispatch helpers.
3. Integration test: middleware mutates messages before upstream.
4. Document in [`embedder_patterns.md`](../guide/embedder_patterns.md).

**Issue:** [`dev/issues/02-host-middleware-hooks.md`](../dev/issues/02-host-middleware-hooks.md)

**Depends on:** Phase 1 recommended (ergonomic `gateway_fields`).

### Phase 4 — Optional follow-ups

| Item | Status |
| --- | --- |
| R3 metadata → headers | **Done** — `forward_metadata_as_headers` on `Endpoint` / `ProviderPool` |
| R5 session reference | **Done** — `unigateway-session` crate (opt-in; `http` feature for routes) |
| R7 usage cache | **Done** — [`usage-cache.md`](../guide/usage-cache.md) |

Phases 1–3 (R1+R2, R6, R4) are implemented in the main workspace crates.

## Verification checklist (release)

- [x] `cargo test --workspace` passes
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] `cargo fmt --all -- --check` clean
- [x] New tests cover `_` strip and `gateway_fields` read path
- [x] CHANGELOG notes `_` field upstream behavior
- [x] `protocol-conversion.md` and embedder docs updated
- [x] Example passthrough works for `stream: true` and `stream: false`

## Example consumer mapping (illustrative)

Any agent product can use the same primitives; names below are **illustrative only**—UniGateway does not implement them.

| Consumer need | UniGateway mechanism |
| --- | --- |
| Body context must not reach OpenAI | R1 + R2 |
| Gateway reads context for delta assembly | R2 + R4 (+ optional R5 reference) |
| Publish / epoch / pinned prefix | Embedder-owned store, or R5 reference |
| Streaming proxy + multi-provider | R6 + existing core |
| Cached token observability | R7 / existing `TokenUsage.cache_hit_tokens` |

## Related documents

- [`arch.md`](arch.md) — library layers and request flow
- [`protocol-conversion.md`](protocol-conversion.md) — neutral model and preservation channels
- [`embedder_patterns.md`](../guide/embedder_patterns.md) — production embedding patterns
- [`usage-cache.md`](../guide/usage-cache.md) — prompt cache token normalization (R7)
- [`unigateway-session/README.md`](../../unigateway-session/README.md) — session prefix reference (R5)
- [`dev/issues/`](../dev/issues/README.md) — GitHub issue drafts
