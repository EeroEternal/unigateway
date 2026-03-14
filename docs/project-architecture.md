## UniGateway Project Architecture and Modules

### 1. Project Scope and Layout

**Scope**: Lightweight open-source LLM gateway + CLI, OpenAI and Anthropic compatible:
- Unified HTTP gateway: `/v1/chat/completions`, `/v1/messages`
- Multi-provider routing and model mapping
- Gateway API keys with quota / QPS / concurrency limits
- Scenario-oriented CLI (clap): service / provider / api-key / metrics
- Optional SDK for programmatic callers

**Crate layout (CLI-first)**:
- No `lib.rs`: single binary only.
- Entry: `src/main.rs` (declares mods, parses CLI; no subcommand → `app::run(config)`).
- `src/app.rs`: `run(config)` and route registration (JSON API + gateway only); re-exports `storage::hash_password`.
- `AppConfig` and `from_env()` in `src/types.rs`; gateway and admin logic in single-file modules: `gateway`, `storage`, `provider`, `service`, `api_key`, `system`, `authz`, `dto`, `queries`, `mutations`.
- Web UI removed; management via CLI and `/api/admin/*` JSON only.

### 2. Entry and Config

#### 2.1 Binary entry `src/main.rs`

CLI (clap) provides a single entry for “scenario management + gateway start”:

- `unigateway serve [--bind] [--db]`: start HTTP gateway (JSON API only, no Web UI)
- `unigateway quickstart`: one-shot default service / provider / api-key (for scripts or AI)
- `unigateway service ...`: manage services (list / create / delete, etc.)
- `unigateway provider ...`: manage providers (list / add / delete / bind, etc.)
- `unigateway api-key ...`: manage API keys (list / create / revoke, etc.)
- `unigateway metrics`: print or export metrics from DB

Subcommands should support machine-readable output (e.g. `--format json`) for AI/automation.

#### 2.2 Config `AppConfig`

File: `src/types.rs`

- Fields: `bind`, `db_url`, `enable_ui` (kept for compatibility; no UI), `admin_token`, `openai_*` / `anthropic_*` (default upstream base URL, API key, model).
- `from_env()`: read from env with sensible defaults.

#### 2.3 Main flow `run(config)`

File: `src/app.rs`

Steps:
- If using SQLite and DB file does not exist, create it.
- Init DB: `SqlitePoolOptions::new().max_connections(5).connect(&config.db_url)`.
- Schema and seed: `storage::init_db(&pool).await?`; create `admin / admin123` if missing.
- Build `AppState`: `pool`, `config`, `api_key_runtime` (per-key QPS/concurrency state), `service_rr` (round-robin index).
- Build Axum routes (JSON API + gateway only): `/health`, `/metrics`, `/v1/models`, `/api/admin/*`, `/v1/chat/completions`, `/v1/messages`.
- Attach state, `TraceLayer`, then `TcpListener::bind` + `axum::serve`.

### 3. CLI module `src/cli.rs`

Role: scriptable operations without Web UI (DevOps / CI).

Functions:
- `init_admin(db_url, username, password)`: create `users` if needed; upsert password via `hash_password`.
- `create_service(db_url, service_id, name)`: ensure admin schema; `INSERT OR REPLACE INTO services(...)`.
- `create_provider(...) -> provider_id`: insert provider (endpoint_id/base_url/model_mapping optional); return id for binding.
- `bind_provider(db_url, service_id, provider_id)`: insert into `service_providers`.
- `create_api_key(...)`: upsert in `api_keys` and `api_key_limits` (QPS/concurrency).
- `print_metrics_snapshot(db_url)`: print `request_stats` counts.

### 4. Protocol and upstream `src/protocol.rs`

Role: adapt gateway JSON to `llm_connector::ChatRequest / ChatResponse` and call upstream.

- `UpstreamProtocol`: `OpenAi` / `Anthropic`.
- `openai_payload_to_chat_request` / `anthropic_payload_to_chat_request`: parse payload into `ChatRequest`.
- `invoke_with_connector(protocol, base_url, api_key, req)`: build `LlmClient`, call `client.chat(req).await`.
- `chat_response_to_openai_json` / `chat_response_to_anthropic_json`: convert `ChatResponse` to gateway JSON.

This layer hides upstream SDK differences and exposes a single `ChatRequest` / `ChatResponse` view.

### 5. SDK `src/sdk.rs`

Role: simple HTTP client for downstream services.

- `UniGatewayClient`: `base_url`, optional `api_key`, `reqwest::Client`.
- `openai_chat(&self, payload)`: POST `/v1/chat/completions` with Bearer if needed.
- `anthropic_messages(&self, payload)`: POST `/v1/messages` with `anthropic-version` and `x-api-key`.

Callers build OpenAI/Anthropic-style `serde_json::Value` and pass to the SDK.

### 6. Storage and gateway logic

#### 6.1 App state `src/types.rs`

- `AppState`: `pool`, `config`, `api_key_runtime`, `service_rr`.
- `GatewayApiKey`, `ServiceProvider`, `RuntimeRateState`, `LoginForm`, `ModelList` / `ModelItem`.

#### 6.2 Storage helpers `src/storage.rs`

- Init all tables (users, sessions, request_stats, services, providers, api_keys, limits, request_logs).
- `init_db(pool)`, `hash_password(raw)`, `record_stat(...)`.
- `find_gateway_api_key(pool, raw_key)`: join `api_keys` + `api_key_limits`, return `GatewayApiKey`.
- `select_provider_for_service(state, service_id, protocol)`: choose provider from bindings using `service_rr` round-robin.
- `map_model_name(model_mapping, requested_model)`: JSON or string mapping.

#### 6.3 Gateway handlers `src/gateway.rs`

- `openai_chat(...)`, `anthropic_messages(...)`.

Flow (same for both; only protocol/headers differ):
1. Parse credentials from headers (Bearer or x-api-key); fallback to env keys.
2. Build `ChatRequest` via `*_payload_to_chat_request`.
3. If token present: `find_gateway_api_key`; if gateway key: check active, quota, then `acquire_runtime_limit`, `select_provider_for_service`, resolve base_url/api_key, `map_model_name`, increment `used_quota`.
4. Resolve upstream base_url (provider or env) and api_key.
5. `invoke_with_connector(...)`.
6. `record_stat`; on success convert to OpenAI/Anthropic JSON; on error return 4xx/5xx + JSON; if gateway key, `release_runtime_inflight`.

**Rate limiting**: per-key `window_started_at`, `request_count`, `in_flight`; 1s window; enforce qps_limit and concurrency_limit; 429 when exceeded.

### 7. Management: CLI + JSON API

- Primary: CLI subcommands for service / provider / api-key / metrics CRUD.
- `/api/admin/...` kept for remote or AI-driven management.
- Shared: `dto.rs` (request/response/Row), `queries.rs` (read), `mutations.rs` (write). CLI and HTTP handlers both use these and only differ in I/O format.

### 8. Admin auth (current)

- No Web UI or session. Admin API uses **x-admin-token**: if `UNIGATEWAY_ADMIN_TOKEN` is set, request must send same value; otherwise allow.
- `users` / `sessions` still exist (e.g. InitAdmin writes `users`) but are not used for HTTP admin auth.
- `authz::is_admin_authorized`: token check only.

### 9. Data model (main tables)

- `users`, `sessions`, `request_stats`, `services`, `providers`, `service_providers`, `api_keys`, `api_key_limits`, `request_logs`.

### 10. Guidelines for future work

- Gateway core: extend only in `gateway.rs`, `storage.rs`, `protocol.rs`; keep quota/limit logic there.
- Admin: expose CRUD via CLI and `/api/admin/*`; put new queries/writes in `queries.rs` / `mutations.rs`.
- CLI / SDK: new non-UI operations as subcommands in `cli.rs`; if SDK gains more protocols (e.g. embeddings), keep them aligned with gateway routes.

Use this doc as an architecture map: decide which layer (gateway / admin / SDK / CLI) a feature belongs to, then place it in the right module.
