# Reasoning Normalization Guide

This guide describes how UniGateway handles reasoning-like upstream outputs without baking
provider-specific logic into the library workspace.

The short version is:

- UniGateway owns a small, protocol-neutral normalization contract.
- Consumer applications own provider-specific knowledge and drift handling.
- Unknown or undeclared inputs must remain plain text.

That boundary keeps UniGateway reusable while still letting embedders bridge noisy provider
behavior into downstream protocols such as Anthropic Messages.

---

## 1. Design Goals

UniGateway should be able to:

- preserve native structured reasoning when the upstream protocol already exposes it;
- rebuild downstream protocol shapes from explicitly declared text encodings when that is safe;
- avoid provider-name branches in core, host, or protocol crates;
- fall back to plain text whenever the input is unknown or ambiguous.

UniGateway should not try to:

- guess provider behavior from brand names;
- infer reasoning from arbitrary text heuristics;
- absorb product-shell provider profiles or operator policy.

---

## 2. Responsibility Split

### UniGateway responsibilities

UniGateway owns the neutral contract:

- detect native structured reasoning fields when the upstream already exposes them;
- accept an explicitly declared reasoning text encoding from request or endpoint metadata;
- apply a conservative built-in decoder for known encodings;
- preserve a loss-aware distinction between native reasoning, rebuilt reasoning, and plain text.

### Consumer application responsibilities

The application embedding UniGateway owns provider-specific knowledge:

- observe or test how a given provider, endpoint, or model actually returns reasoning;
- decide whether an endpoint should advertise a declared encoding;
- inject metadata into endpoint definitions or per-request overrides;
- handle provider-specific drift when a vendor changes behavior;
- wrap a provider with a custom driver or adapter if richer normalization is required.

This split is intentional. Provider behavior is operational knowledge and can drift over time;
it does not belong in UniGateway's generic library boundary.

---

## 3. Normalization Order

UniGateway should interpret upstream outputs in this order:

1. Native structured reasoning from the upstream protocol.
2. Explicit structured reasoning fields on OpenAI-compatible payloads, such as
   `reasoning_content` or `thinking`.
3. Explicitly declared text encodings from metadata.
4. Plain text fallback.

If an upstream response does not match one of the first three cases, UniGateway must treat it
as ordinary text.

This rule matters more than any individual parser. The safety property is that uncertain input
never gets upgraded into structured reasoning.

---

## 4. Current Built-In Contract

UniGateway currently exposes a neutral metadata contract for declared text encodings:

```rust
use unigateway_sdk::protocol::{
    REASONING_TEXT_ENCODING_KEY,
    REASONING_TEXT_ENCODING_XML_THINK_TAG,
};
```

The current built-in encoding value is:

- `xml_think_tag`: a prefixed `<think>...</think>` text segment.

The parser is intentionally strict:

- the tag must appear at the start of the text payload;
- the closing tag must be present;
- incomplete streaming fragments buffer until the parser can decide safely;
- if the structure never completes, the buffered content falls back to plain text.

When UniGateway rebuilds Anthropic `thinking` blocks from declared text encodings, the emitted
signature is a placeholder for protocol-shape compatibility only. It is not a verbatim
Anthropic signature.

### Backward compatibility

UniGateway still accepts the legacy metadata key:

- `unigateway.anthropic_reasoning_text_format`

New integrations should prefer the neutral key:

- `unigateway.reasoning_text_encoding`

---

## 5. How Consumers Should Apply It

### Option A: Declare endpoint behavior statically

If a specific endpoint is known to emit a declared text encoding, set metadata when building the
endpoint or pool.

```rust
use std::collections::HashMap;
use unigateway_sdk::core::{
    Endpoint, LoadBalancingStrategy, ModelPolicy, ProviderKind, ProviderPool, RetryPolicy,
    SecretString,
};
use unigateway_sdk::protocol::{
    REASONING_TEXT_ENCODING_KEY,
    REASONING_TEXT_ENCODING_XML_THINK_TAG,
};

let endpoint = Endpoint {
    endpoint_id: "compat-openai".to_string(),
    provider_name: Some("compat-openai".to_string()),
    source_endpoint_id: Some("compat-openai".to_string()),
    provider_family: Some("openai-compatible".to_string()),
    provider_kind: ProviderKind::OpenAiCompatible,
    driver_id: "openai-compatible".to_string(),
    base_url: "https://example.invalid/v1".to_string(),
    api_key: SecretString::new("sk-...".to_string()),
    model_policy: ModelPolicy::default(),
    enabled: true,
    metadata: HashMap::from([(
        REASONING_TEXT_ENCODING_KEY.to_string(),
        REASONING_TEXT_ENCODING_XML_THINK_TAG.to_string(),
    )]),
};

let pool = ProviderPool {
    pool_id: "svc".to_string(),
    endpoints: vec![endpoint],
    load_balancing: LoadBalancingStrategy::RoundRobin,
    retry_policy: RetryPolicy::default(),
    metadata: HashMap::new(),
};
```

Use this when your application maintains an operator-owned provider profile.

### Option B: Override per request

If the same endpoint can vary by model, feature flag, or request knob, attach the metadata at
request time instead.

```rust
use std::collections::HashMap;
use unigateway_sdk::core::{Message, MessageRole, ProxyChatRequest};
use unigateway_sdk::protocol::{
    REASONING_TEXT_ENCODING_KEY,
    REASONING_TEXT_ENCODING_XML_THINK_TAG,
};

let request = ProxyChatRequest {
    model: "claude-opus-4-7".to_string(),
    messages: vec![Message::text(MessageRole::User, "Solve the puzzle")],
    temperature: None,
    top_p: None,
    max_tokens: None,
    stream: true,
    metadata: HashMap::from([(
        REASONING_TEXT_ENCODING_KEY.to_string(),
        REASONING_TEXT_ENCODING_XML_THINK_TAG.to_string(),
    )]),
};
```

This is the right layer when the embedder owns routing policy, request enrichment, or model
selection logic.

---

## 6. What Consumers Should Do For Provider-Specific Cases

If a provider needs custom handling beyond UniGateway's small built-in contract, keep that logic
in the consumer application.

Recommended patterns:

1. Maintain a provider profile in your application or control plane.
2. Project that profile into endpoint metadata or request metadata.
3. If the provider needs request-dependent policy, inject or override metadata before dispatch.
4. If the provider needs deeper normalization than metadata can express, wrap it with a custom
    driver or adapter before its output enters UniGateway's generic response rendering path.

This keeps UniGateway neutral while still allowing the embedder to encode operational
knowledge.

### Recommended escalation ladder

Choose the smallest extension point that can express the provider-specific behavior you need:

1. Endpoint metadata profile.
    Use this when the behavior is stable for a specific endpoint or provider profile.

2. Per-request metadata override.
    Use this when the behavior depends on model, prompt mode, tool usage, or another request knob.

3. `GatewayHooks::on_request`.
    Use this when the consumer application needs to inject, rewrite, or remove request metadata
    before execution without forking UniGateway's request parsing or rendering code.

4. `HostDispatchTarget::Pool` or `HostDispatchTarget::PoolRef`.
    Use this when the consumer application wants to supply a fully materialized pool or endpoint
    set after applying product-specific policy, routing, or provider profiling.

5. Custom upstream driver or adapter.
    Use this when the upstream wire format itself needs provider-specific parsing or translation
    that should happen before UniGateway's neutral response renderers see the payload.

This ordering is deliberate. The first four options keep the provider-specific knowledge in the
consumer application while still letting UniGateway execute and render through its generic
contracts. A custom driver is the last step, not the first step.

### Decision table

| Situation | Recommended owner | Recommended extension point |
| --- | --- | --- |
| Endpoint always emits a stable declared reasoning text format | Consumer app | Endpoint metadata |
| Same endpoint changes behavior by model or request mode | Consumer app | Request metadata or `GatewayHooks::on_request` |
| Consumer app already performs explicit routing or provider selection | Consumer app | `HostDispatchTarget::Pool` / `PoolRef` |
| Upstream payload needs provider-specific parsing before it looks like a normal OpenAI- or Anthropic-shaped response | Consumer app | Custom driver / adapter |
| No stable provider knowledge exists yet | Consumer app | Treat as `unknown`, keep plain text |

### Why there is no generic provider hook in UniGateway today

UniGateway already exposes enough neutral integration points for the consumer application to own
provider specialization:

- request metadata;
- endpoint metadata;
- `GatewayHooks::on_request`;
- explicit host dispatch with `HostDispatchTarget::Pool` and `PoolRef`;
- custom drivers.

Adding a first-class `provider_normalizer` hook inside UniGateway itself would risk shifting
provider knowledge back into the generic library boundary. The current design instead keeps
specialization on the embedder side and lets UniGateway consume the resulting neutral metadata or
payload shape.

---

## 7. Failure Semantics

Embedders should expect and rely on these fallback rules:

- no declared encoding -> plain text;
- malformed declared encoding -> plain text;
- incomplete stream that never closes the declared block -> plain text;
- native structured reasoning always wins over declared text encodings.

This is the core safety rule. UniGateway prefers false negatives over false positives because a
missed reasoning block is less harmful than incorrectly reclassifying ordinary text.

---

## 8. Recommended Operational Model

Treat provider behavior as one of three states:

- `declared`: your application has an explicit profile and attaches metadata;
- `observed`: your application has probed the provider recently, but the behavior is not yet a
  stable contract;
- `unknown`: no explicit knowledge is available.

Only the `declared` state should influence UniGateway metadata by default. `observed` and
`unknown` should remain operational information in the consumer application unless an operator
promotes them into an explicit profile.

That approach makes provider drift visible and avoids hard-coding temporary behavior into the
generic library workspace.

---

## 9. Embedder Rollout Checklist

This section translates the neutral contract into a host-application implementation plan.

### Path A: Provider profile in the consumer application

Pick this path when the provider behavior is stable enough to describe as configuration.

Checklist:

1. Create a provider-profile record in your application or control plane.
2. Store the behavior at the smallest stable granularity you trust.
    Prefer `endpoint + model family + mode` over only `provider name`.
3. Encode only declared behavior.
    Do not promote `observed` behavior into live metadata until an operator or maintainer accepts it.
4. Project the profile into `Endpoint.metadata` when you build or refresh pools.
5. Set `REASONING_TEXT_ENCODING_KEY` only for endpoints that are explicitly known to use a
    supported text encoding.
6. Leave unknown endpoints unannotated so UniGateway falls back to plain text.
7. Re-probe the provider periodically and downgrade the profile if the behavior drifts.

Use this path when you want the cleanest operational model and the least request-time logic.

### Path B: Request enrichment in the consumer application

Pick this path when the same endpoint behaves differently by model, request mode, tools, or other
runtime knobs.

Checklist:

1. Keep endpoint metadata minimal and generic.
2. Before dispatch, decide whether the current request should advertise a reasoning text encoding.
3. Attach or remove `REASONING_TEXT_ENCODING_KEY` on `ProxyChatRequest.metadata`.
4. If the policy belongs to request middleware rather than the call site, implement it in
    `GatewayHooks::on_request`.
5. Make the rule deterministic and explainable.
    For example: "only model X with mode Y gets encoding Z".
6. If the policy cannot make a clear decision, send no encoding metadata and let UniGateway keep
    the content as plain text.

Use this path when provider behavior is real but conditional, and you do not want to explode the
number of endpoint profiles.

### Path C: Custom driver or adapter in the consumer application

Pick this path when the upstream protocol itself needs provider-specific parsing before it even
resembles a normal OpenAI- or Anthropic-shaped response.

Checklist:

1. Confirm that metadata is not enough.
    If the provider can already be expressed as a declared encoding or request-time policy, do not
    jump to a custom driver.
2. Wrap the upstream protocol at the consumer-application boundary.
3. Translate provider-private request or stream shapes into a neutral OpenAI-compatible or
    Anthropic-compatible shape before UniGateway's generic renderer sees them.
4. Preserve raw evidence in your adapter logs so provider drift is debuggable.
5. Keep the adapter owned by the consumer application, not by UniGateway.
6. Document which provider behaviors the adapter normalizes and which still fall back to plain
    text.

Use this path only when the provider shape is too custom to fit the metadata-based neutral
contract.

### How to choose quickly

Use this rule of thumb:

- Stable per-endpoint behavior -> provider profile.
- Stable but request-dependent behavior -> request enrichment.
- Provider-private wire format -> custom driver or adapter.

When in doubt, choose the smaller path first. UniGateway's fallback behavior is intentionally safe,
so it is better to under-normalize than to ship a provider-specific parser too early.