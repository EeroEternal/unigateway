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

**Current**: `service_providers` supports multiple bindings; `providers.weight` exists; `select_provider_for_service` does in-memory round-robin by service_id + protocol. **Gaps**: (a) Docs/CLI don’t advertise “bind multiple → automatic round-robin”; (b) 加权（weight）**暂不支持**；`providers.weight` 字段保留，当前路由未使用。

**Suggestions**: Document that binding multiple Providers gives round_robin.

### 4. Scenario: Multi-Provider Weighted

**User goal**: Two providers (cheap vs expensive); most traffic to cheap, some to expensive; automatic routing, no app logic.

**Target**: Set weight (e.g. A=3, B=1) in CLI; same Service binds both; gateway sends 3:1 to A vs B.

**Status**: **暂不支持**。设计保留：使用 `providers.weight` 做相对权重、在 `select_provider_for_service` 中实现加权轮询；若权重均为 NULL/0 则回退到等权 round-robin。

### 5. Scenario: Simple Fallback（类似 Cloudflare 动态路由）

**User goal**: Primary + backup provider; on request failure (e.g. 5xx or connection error), automatically try next provider; one route name, automatic fallback.

**Target**: Service 设置 `routing_strategy = "fallback"`；Binding 用 `priority` 排序（0=主，1=第一备份…）；请求时按优先级依次尝试，直到成功或全部失败。

**Current**: (1) Config: Service 支持 `routing_strategy`（默认 `round_robin`，可选 `fallback`）；Binding 支持 `priority`（默认 0，数值越小越优先）。(2) 请求级回退：选主 Provider 调用；若返回 5xx 或连接错误，自动用下一优先级 Provider 重试；仅 5xx/连接错误触发回退，4xx 不重试。(3) 无持久化健康状态，单请求内按顺序尝试。

### 6. Scenario: Embeddings

**User goal**: Chat + embeddings through one gateway; don’t wire each provider separately.

**Target**: Expose `/v1/embeddings` (OpenAI-compatible); route by Service/Provider.

**Design**: Add embeddings adapters in protocol.rs (e.g. openai_embeddings_payload_to_request, embeddings_response_to_openai_json); reuse llm-connector embeddings or extend like Chat. Reuse API Key → Service → select_provider_for_service and model_mapping. No dedicated embeddings UI at first; document supported models in Provider / model_mapping and configure via CLI/JSON.

### 7. Scenario: Another Mainstream Chat Provider

**User goal**: Add e.g. Gemini, Groq, DeepSeek while keeping downstream OpenAI/Anthropic compatible.

**Target**: Add Provider via CLI/JSON (provider_type, endpoint_id); downstream still calls `/v1/chat/completions` or `/v1/messages`; gateway routes by config.

**Design**: provider_type + endpoint_id + base_url + api_key suffice; maintain endpoint metadata in llm_providers. If llm-connector supports the provider, no extra work in protocol.rs; otherwise extend llm-connector and keep gateway choosing endpoint/model. Add a concrete doc example (e.g. “Using DeepSeek/Groq via UniGateway”) with Provider config, binding, and call params.

### 8. High-Value Lightweight Scenarios (Priority)

All stay within “no Redis, no K8s, no extra services”; TOML config + single process only.

- **Local dev / multi-model playground**: One quickstart, multiple providers on one service; one gateway URL/key; switch via routing_strategy (e.g. PriorityFallback, Single). Add `/v1/embeddings` for “chat + vectors”. README: “3-minute local setup” + curl.
- **Small team shared gateway**: Per-key quota, QPS, logs; Weighted/RoundRobin; clear JSON errors (e.g. “quota exceeded”); quickstart `--team` to create several keys; `.env.example` and “team gateway” template.
- **RAG / knowledge-base app**: One gateway for chat + embeddings; route by model (e.g. embeddings → cheap provider, chat → quality provider); add model_pattern if needed; doc “RAG minimal config”.
- **Cost-aware production proxy**: Weighted + PriorityFallback for “cheap first, quality fallback”; v0.2 weight-based split; later optional latency scoring from logs; doc “production cost template” (e.g. 70% cheap / 30% quality).

### 9. Implementation Order

1. **Docs and CLI**: Make single-provider and multi-provider round-robin obvious and easy to try.
2. **Weights**: **暂缓**。加权路由暂不支持；`providers.weight` 字段保留供后续使用。
3. **Fallback**: 已实现请求级主路 + 自动回退（`routing_strategy = "fallback"` + `priority`）。
4. **Embeddings + one more provider**: Add `/v1/embeddings` and one mainstream provider example.

Goal: use a few high-signal scenarios to get the most from current abstractions (Service, Provider, API Key, model mapping, routing strategy) and give a clear direction for later iterations without making the gateway heavy.
