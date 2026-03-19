# llm-connector Requirements: OpenAI Responses API Support

## Context

UniGateway now accepts `/v1/responses` for Codex compatibility, but current upstream provider coverage is mixed:

- Codex CLI uses OpenAI Responses API (`/v1/responses`)
- Some upstream providers do not implement `/v1/responses` (for example, current Moonshot route in this environment)
- Those providers may still support `/v1/chat/completions`

To keep gateway logic thin and reusable, protocol-level Responses support should live in `llm-connector`.

## Goal

Add first-class Responses API support to `llm-connector` with automatic fallback to chat completions when upstream does not support `/v1/responses`, while preserving streaming behavior expected by Codex.

## Non-goals

- Do not break existing chat/completions and Anthropic message paths
- Do not require UniGateway to implement provider-specific Responses conversion logic
- Do not block on full multimodal parity for initial delivery

## P0 (must have)

### 1) Core types

Add normalized connector-level types:

- `ResponsesRequest`
- `ResponsesResponse`
- `ResponsesStreamEvent`

Requirements:

- Keep unknown fields pass-through where possible (serde flatten style)
- Preserve provider-specific metadata without data loss

### 2) Connector API entry points

Add public APIs:

- `invoke_responses(...) -> Result<ResponsesResponse>`
- `invoke_responses_stream(...) -> Result<impl Stream<Item = Result<ResponsesStreamEvent>>>`

Behavior:

- If provider supports `/v1/responses`: direct call
- If provider returns `404`/"not found" for `/responses`: fallback path (below)

### 3) Fallback strategy: responses -> chat/completions

For providers without Responses API, implement deterministic fallback:

- Map `ResponsesRequest` to chat request:
  - `input` -> `messages`
  - `instructions` -> `system` message
  - `model` -> `model`
  - `temperature`, `top_p`, `max_output_tokens` -> equivalent chat fields
  - `stream` preserved
- Execute chat/completions upstream
- Convert chat result back to `ResponsesResponse`

### 4) Streaming compatibility

Support stream mode for both direct and fallback paths.

For fallback path, emit minimal Responses-compatible event sequence:

- `response.created`
- `response.output_text.delta` (repeat)
- `response.completed`

This is enough for Codex basic text output path.

### 5) Error model

Return structured connector errors with clear classification:

- Upstream unsupported endpoint
- Upstream auth failure
- Upstream rate limit
- Mapping/serialization failure

Include diagnostic context:

- provider id/family
- attempted endpoint (`/v1/responses` or fallback)
- status code
- safe body snippet

## P1 (recommended next)

### 1) Tool call mapping

Add bidirectional mapping:

- Responses tool fields <-> chat tool call fields
- Preserve `function.name` and `arguments` exactly

### 2) Usage normalization

Normalize usage fields across direct/fallback:

- `input_tokens`
- `output_tokens`
- `total_tokens`

Allow nullable values when provider omits data.

### 3) Retry semantics

Ensure direct and fallback paths share the same retry/backoff policy as existing connector calls.

## P2 (later)

- Multimodal content parity (image/audio)
- Full reasoning/event parity with OpenAI Responses schema variants
- Extended conversation linkage (`previous_response_id`) best-effort support

## Acceptance criteria

### A) Direct Responses provider

Given an upstream that supports `/v1/responses`:

- Non-stream call succeeds with `ResponsesResponse`
- Stream call yields valid ordered `ResponsesStreamEvent`

### B) Fallback provider

Given an upstream that does not support `/v1/responses` but supports `/v1/chat/completions`:

- Connector automatically falls back
- Non-stream and stream both succeed
- Codex no longer fails with endpoint-404-based retries

### C) Regression safety

- Existing chat/completions connector APIs pass existing tests unchanged
- Existing Anthropic path behavior unchanged

### D) Observability

Connector logs must clearly indicate which path was used:

- direct responses
- fallback to chat
- fallback failure

## Suggested test matrix

1. Unit tests for request mapping (responses -> chat)
2. Unit tests for response mapping (chat -> responses)
3. Integration test: direct `/v1/responses`
4. Integration test: forced `/responses` 404 then fallback success
5. Integration test: stream fallback event ordering
6. Error-path tests for auth/rate-limit/invalid payload

## Integration contract for UniGateway

After this is implemented in `llm-connector`, UniGateway should only:

- Route `/v1/responses` requests
- Perform auth, mode/provider selection, model mapping
- Delegate protocol execution to connector responses APIs

No provider-specific Responses conversion logic should remain in gateway layer.
