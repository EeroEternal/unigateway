# Internal Refactor Plan: Render Helpers, Execution Loop, Single-Source Field Lists

Status: approved and implemented (2026-08); shipped in the 2.15.0 series. Scope decision record for the 2.15.x
internal-quality series.

## Goals

Reduce structural duplication inside `unigateway-core` so that adding a new
request kind or a new protocol has a low marginal cost — **without any
breaking change to the public API**. Embedders on 2.14.x must be able to bump
to 2.15.x without code changes:

* public function signatures unchanged,
* rendered upstream payload bytes unchanged (locked by the golden
  render-determinism tests added in 2.14.2),
* metadata key strings unchanged.

## Work Items

### PR-1 ✅ — Shared render helpers + metadata key constants

Duplicated mechanical helpers across renderers move to one place:

* `resolved_model()` and `join_url()` exist identically in both
  `protocol/openai/requests.rs` and `protocol/anthropic/requests.rs`; they
  become methods on `DriverEndpointContext`.
* The duplicated header builders collapse into one helper parameterized by the
  auth header format.
* The thrice-copied extra-merge loop delegates to the existing
  `merge_forwardable_extra()`.
* All `"unigateway.*"` metadata key string literals (~25 distinct keys) become
  named constants in one module; raw literals disappear from source.

### PR-2 ✅ — Generic engine execution loop

`engine/execution/{chat,responses,embeddings}.rs` are ~85% identical copies of
the AIMD acquire → driver context → attempt events → fallback loop (~830 lines
total). They consolidate onto one generic private skeleton; the three public
entry points (`proxy_chat`, `proxy_responses`, `proxy_embeddings`) keep their
exact signatures. Expected net reduction ~500 lines. Guarded by the existing
engine test suite.

### PR-3 ✅ — Known-field single source of truth

The hand-written field lists in `unigateway-protocol/src/requests.rs`
(`is_openai_chat_known_field`, `is_anthropic_chat_known_field`) decide which
payload fields are typed, which land in `extra`, and which are gateway-only.
The core renderers hold their own implicit notion of "core fields". Drift
between the two caused the Responses `_`-field leak fixed in 2.14.2. Each
protocol gets one `const KNOWN_FIELDS` referenced by both sides, plus a parse →
re-render round-trip invariant test.

### PR-4 ✅ — Split `unigateway-config/src/admin.rs`

1104 lines mixing mutation dispatch, view building, and API-key management
split into `admin/{mutations,views,api_keys}.rs`. The module is already
private (`mod admin`); `GatewayState` method signatures stay frozen.

## Explicitly Deferred

* **ProviderDriver trait hierarchy**: only two real drivers exist (+ sglang
  delegating to OpenAI-compatible). No abstraction until a third real driver
  appears.
* **SSE renderer unification** in `unigateway-protocol`: wait for a third
  streaming path.
* **Typed config mutation API** (`ConfigMutation` enum): would change the
  public surface of `GatewayState`; only via additive entry point +
  `#[deprecated]` on the old one across ≥2 minor versions.
* **Neutral-model reshaping**: `ProxyChatRequest` stays Anthropic-flavored;
  see below.

## Direction Decision: OpenAI-as-Hub for Future Protocols

When a fourth client protocol arrives (e.g. Gemini), it implements **only**
`X → OpenAI-shape` and `OpenAI-shape → X` conversions, instead of pairwise
N² conversions. Three guardrails:

1. **Same-protocol passthrough stays byte-preserving** (`raw_messages`
   direct clone for OpenAI, raw pass-through for Anthropic). Cross-protocol
   traffic may route through conversion; same-protocol traffic never does —
   upstream prefix caches depend on byte stability (locked by the golden
   tests).
2. **Hub is a conversion-routing center, not a model rewrite.** The public
   typed model is not reshaped toward OpenAI semantics; conversions continue
   to operate at the `raw_messages: Value` layer like today's
   `openai_messages_to_anthropic_messages` pair.
3. **Lossy-conversion policies live in one place** (conversion layer):
   placeholder thinking signatures gated by capabilities, tool_use/tool_result
   pairing, system-prompt position mapping. New spokes reuse them rather than
   re-implementing.

Marginal cost target for a new protocol after A-items complete: one conversion
function pair + payload parsing + possibly reuse of `OpenAiCompatibleDriver`.

## Release Discipline

All items are internal refactors or additive API: semver-minor bumps
(2.15.x). Upgrader-facing statement: no signature changes, no byte changes,
no metadata key changes; some new methods/constants on `DriverEndpointContext`.
