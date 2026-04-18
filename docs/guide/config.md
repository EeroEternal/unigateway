# Config file format (TOML)

UniGateway uses a single TOML file (default `unigateway.toml`) for services, providers, bindings, and API keys.

This file is the persisted product configuration, not the direct execution state. At runtime, the product shell projects the config into `unigateway-core::ProviderPool` values via `src/config/core_sync.rs`, and those pools are stored inside `UniGatewayEngine`.

The file is loaded at startup and written back when configuration changes (for example via CLI or Admin API). Request counts and in-flight rate state are memory-only and are not stored in the file.

## Example

```toml
[[services]]
id = "default"
name = "Default"

[[providers]]
name = "openai-prod"
provider_type = "openai"
endpoint_id = "openai"
base_url = "https://api.openai.com"
api_key = "sk-..."
model_mapping = ""
is_enabled = true

[[bindings]]
service_id = "default"
provider_name = "openai-prod"

[[api_keys]]
key = "ugk_abc123..."
service_id = "default"
quota_limit = 100000
used_quota = 0
is_active = true
qps_limit = 20.0
concurrency_limit = 8
```

## Fields

- **services**: `id` (string), `name` (string), `routing_strategy` (optional, default `"round_robin"`, can be `"fallback"`).
- **providers**: `name`, `provider_type`, `endpoint_id`, `base_url`, `api_key`, `model_mapping` (optional), `is_enabled` (default true). The order defines `provider_id` (0-based index) for bindings.
- **bindings**: `service_id`, `provider_name` (must match a provider’s `name`), `priority` (optional, default 0; lower = higher priority, used when `routing_strategy = "fallback"`).
- **api_keys**: `key`, `service_id`, `quota_limit` (optional), `used_quota` (updated at runtime), `is_active`, `qps_limit` (optional), `concurrency_limit` (optional).

## How Config Becomes Execution State

At runtime, UniGateway projects config objects into core engine objects like this:

- one `service` -> one `ProviderPool`
- one `binding` + one `provider` -> one core `Endpoint`
- one `api_key.service_id` -> selects which service / pool a request uses

Important mapping details from `src/config/core_sync.rs`:

- `service.id` becomes `ProviderPool.pool_id`
- `service.routing_strategy` maps to core `LoadBalancingStrategy`
- `provider.provider_type` decides `provider_kind` and `driver_id`
- `provider.default_model` and `provider.model_mapping` become `ModelPolicy`
- provider metadata such as `provider_name`, `source_endpoint_id`, `provider_family`, and `binding_priority` are copied into endpoint metadata

If a provider is disabled, missing an API key, or cannot resolve its upstream base URL, the service may fail core sync and become unavailable for execution.

## Routing Semantics

The persisted config model and the execution model are related but not identical:

- product layer uses `service`, `provider`, `binding`, and `mode`
- core layer uses `pool`, `endpoint`, and `ExecutionTarget`

For most requests, a gateway API key selects a `service_id`, and that service maps to the core pool with the same id.

## Persistence Notes

- The config file is durable.
- Rate-limit windows, request counters, and in-flight counts are in-memory only.
- Dirty config state is persisted periodically and during graceful shutdown.

You can edit the file by hand and restart the gateway, or use the CLI / Admin API to mutate and persist.
