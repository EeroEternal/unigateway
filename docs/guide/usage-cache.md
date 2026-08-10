# Prompt cache usage fields

UniGateway normalizes upstream **prompt cache** token counts into `TokenUsage.cache_hit_tokens` and `TokenUsage.cache_write_tokens`. Embedders can read these fields from core reports and from protocol-rendered OpenAI / Anthropic responses.

## Normalized fields

| Field | Meaning |
| --- | --- |
| `cache_hit_tokens` | Tokens served from provider prompt cache (read / hit) |
| `cache_write_tokens` | Tokens written into provider prompt cache (creation) |

Both are `Option<u64>` on [`TokenUsage`](../../unigateway-core/src/response.rs). Missing or unparsable upstream fields remain `None`; parsing never fails the request.

## OpenAI-compatible upstream mappings

Chat and Responses usage parsing (`unigateway-core/src/protocol/openai/parsing.rs`) resolves `cache_hit_tokens` from the first matching location:

1. `usage.cache_hit_tokens`
2. `usage.input_tokens_details.cached_tokens`
3. `usage.prompt_tokens_details.cached_tokens`
4. `usage.prompt_cache_hit_tokens` (DeepSeek-style)
5. `usage.cached_tokens` (Qwen-style top-level)

`cache_write_tokens` is resolved from:

1. `usage.cache_write_tokens`
2. `usage.cache_creation_input_tokens`
3. `usage.prompt_tokens_details.cache_creation_input_tokens`

Existing explicit `cache_hit_tokens` in upstream JSON is preserved (not overwritten by nested fields).

## Client-visible rendering

OpenAI Chat render paths expose normalized values when present, for example in completed JSON:

```json
{
  "usage": {
    "prompt_tokens": 100,
    "completion_tokens": 20,
    "total_tokens": 120,
    "cache_hit_tokens": 80
  }
}
```

Anthropic render paths map equivalent usage where available. Raw upstream usage JSON remains in response `raw` payloads for embedders that need vendor-specific fields.

## Embedder guidance

- Use `TokenUsage.cache_hit_tokens` for observability / billing hooks (`GatewayHooks`, request reports).
- Do not depend on a single vendor key in upstream JSON; prefer the normalized core field.
- Zero cache hits are represented as `Some(0)` when upstream sends explicit zero.

## Related

- [`embedder-neutral-extensions.md`](../design/embedder-neutral-extensions.md) — R7 scope
- [`protocol-conversion.md`](../design/protocol-conversion.md) — response normalization layers
