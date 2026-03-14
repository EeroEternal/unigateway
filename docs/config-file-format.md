# Config file format (TOML)

UniGateway uses a single TOML file (default `unigateway.toml`) for services, providers, bindings, and API keys. The file is loaded at startup and written back when configuration changes (e.g. via CLI or Admin API). Request counts are kept in memory only and are not stored in the file.

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
weight = 1
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

- **services**: `id` (string), `name` (string).
- **providers**: `name`, `provider_type`, `endpoint_id`, `base_url`, `api_key`, `model_mapping` (optional), `weight` (default 1), `is_enabled` (default true). The order defines `provider_id` (0-based index) for bindings.
- **bindings**: `service_id`, `provider_name` (must match a provider’s `name`).
- **api_keys**: `key`, `service_id`, `quota_limit` (optional), `used_quota` (updated at runtime), `is_active`, `qps_limit` (optional), `concurrency_limit` (optional).

You can edit the file by hand and restart the gateway, or use the CLI / Admin API to mutate and persist.
