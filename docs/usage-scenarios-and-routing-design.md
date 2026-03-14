## UniGateway Scenario-Driven Routing and Adapter Design

This doc starts from common usage scenarios to constrain abstractions and implementation order, so the gateway stays light while gaining expressiveness.

### 1. Concepts (and Current Implementation)

**Service**: Downstream-facing “logical service”; one Service can bind multiple Providers; unit of routing and stats; has `routing_strategy`, default `round_robin`.

**Provider**: Upstream config: type (openai, anthropic, etc.), endpoint_id, base_url, api_key, model_mapping. A Provider can be bound to multiple Services.

**API Key (gateway key)**: Downstream credential; bound to a Service; quota_limit, qps_limit, concurrency_limit for tenant control.

**Model mapping**: Map downstream model name to upstream model; supports JSON or simple string.

**Routing (implemented)**: Resolve API Key → Service → choose one Provider from bindings (currently round-robin); record request_stats and request_logs.

### 2. Scenario: Single-Provider Direct

**User goal**: Put one provider (e.g. OpenAI) behind the gateway; one gateway URL and one gateway key.

**Target**: Create Service + Provider, bind, create API Key; then call `/v1/chat/completions` with that key; traffic goes to that Provider.

**Gap**: Abstraction already fits (one Service, one Provider). Missing: minimal onboarding path and clear docs: (a) recommended flow via CLI/JSON (create provider, service, bind, create key, call API); (b) one-command scriptable flow (e.g. quickstart) for ops or AI tooling. Changes are mostly docs/UX.

### 3. Scenario: Multi-Provider Round-Robin

**User goal**: Several similar providers; simple multi-active load sharing; round-robin is enough.

**Target**: Bind multiple Providers to one Service; default routing_strategy; requests round-robin across Providers.

**Current**: `service_providers` supports multiple bindings; `providers.weight` exists; `select_provider_for_service` does in-memory round-robin by service_id + protocol. **Gaps**: (a) Docs/CLI don’t advertise “bind multiple → automatic round-robin”; (b) weight not yet used in routing.

**Suggestions**: Document that binding multiple Providers gives round_robin; then add minimal weight support (e.g. expand-by-weight or weighted round-robin) using `providers.weight`.

### 4. Scenario: Multi-Provider Weighted

**User goal**: Two providers (cheap vs expensive); most traffic to cheap, some to expensive; automatic routing, no app logic.

**Target**: Set weight (e.g. A=3, B=1) in CLI; same Service binds both; gateway sends 3:1 to A vs B.

**Design**: Use `providers.weight` as relative weight; in `select_provider_for_service` implement weighted round-robin (e.g. expand to logical array or classic algorithm). If all weights NULL/0, fall back to equal round-robin.

### 5. Scenario: Simple Fallback

**User goal**: Primary + backup provider; on sustained errors, switch to backup; no heavy health system.

**Target**: Multiple Providers, one primary; after N consecutive errors, prefer fallback.

**Design**: (1) Config: reuse weight for soft priority or add light field (e.g. is_primary / priority). (2) In-process: per-Provider error window (e.g. last N calls); over threshold, exclude from candidates for a few seconds; state in memory. (3) Only 5xx/connection errors count; 4xx not provider failure; if all unavailable, try primary again and return clear error.

### 6. Scenario: Embeddings

**User goal**: Chat + embeddings through one gateway; don’t wire each provider separately.

**Target**: Expose `/v1/embeddings` (OpenAI-compatible); route by Service/Provider.

**Design**: Add embeddings adapters in protocol.rs (e.g. openai_embeddings_payload_to_request, embeddings_response_to_openai_json); reuse llm-connector embeddings or extend like Chat. Reuse API Key → Service → select_provider_for_service and model_mapping. No dedicated embeddings UI at first; document supported models in Provider / model_mapping and configure via CLI/JSON.

### 7. Scenario: Another Mainstream Chat Provider

**User goal**: Add e.g. Gemini, Groq, DeepSeek while keeping downstream OpenAI/Anthropic compatible.

**Target**: Add Provider via CLI/JSON (provider_type, endpoint_id); downstream still calls `/v1/chat/completions` or `/v1/messages`; gateway routes by config.

**Design**: provider_type + endpoint_id + base_url + api_key suffice; maintain endpoint metadata in llm_providers. If llm-connector supports the provider, no extra work in protocol.rs; otherwise extend llm-connector and keep gateway choosing endpoint/model. Add a concrete doc example (e.g. “Using DeepSeek/Groq via UniGateway”) with Provider config, binding, and call params.

### 8. High-Value Lightweight Scenarios (Priority)

All stay within “no Redis, no K8s, no extra services”; SQLite + single process only.

- **Local dev / multi-model playground**: One quickstart, multiple providers on one service; one gateway URL/key; switch via routing_strategy (e.g. PriorityFallback, Single). Add `/v1/embeddings` for “chat + vectors”. README: “3-minute local setup” + curl.
- **Small team shared gateway**: Per-key quota, QPS, logs; Weighted/RoundRobin; clear JSON errors (e.g. “quota exceeded”); quickstart `--team` to create several keys; `.env.example` and “team gateway” template.
- **RAG / knowledge-base app**: One gateway for chat + embeddings; route by model (e.g. embeddings → cheap provider, chat → quality provider); add model_pattern if needed; doc “RAG minimal config”.
- **Cost-aware production proxy**: Weighted + PriorityFallback for “cheap first, quality fallback”; v0.2 weight-based split; later optional latency scoring from logs; doc “production cost template” (e.g. 70% cheap / 30% quality).

### 9. Implementation Order

1. **Docs and CLI**: Make “single-provider” and “multi-provider round-robin” obvious and easy to try; no code change required first.
2. **Weights**: Implement minimal weight support in `select_provider_for_service` using `providers.weight`.
3. **Soft fallback**: Simple per-Provider error count in gateway (in-process, no persistence).
4. **Embeddings + one more provider**: Add `/v1/embeddings` and one mainstream provider example; “chat + embeddings + multi-provider” as a flagship combo.

Goal: use a few high-signal scenarios to get the most from current abstractions (Service, Provider, API Key, model mapping, routing strategy) and give a clear direction for later iterations without making the gateway heavy.
