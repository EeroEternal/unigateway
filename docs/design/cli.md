## UniGateway CLI-first Design Draft

### 1. Goals

- Position UniGateway as **HTTP gateway + scenario-oriented CLI**: no Web UI; programmatic admin is limited to `/api/admin/*` (services, providers, bindings, API keys)—mode switching stays in CLI (`ug mode use`, etc.).
- All management (create/list/delete service, provider, api-key; view metrics) via CLI, with:
  - Human-friendly flags and help;
  - Stable subcommands and parseable output for AI/scripts.

### 2. Top-level Commands (clap)

Proposed structure:

- `unigateway serve [FLAGS] [OPTIONS]`
  - Start HTTP gateway.
  - Options: `--bind <ADDR>`, `--config <PATH>` (override env).
- `unigateway quickstart [OPTIONS]`
  - One-shot init: default service (e.g. `default`), default provider (from provider_type/endpoint_id/base_url/api_key), bind, create one API key and print it.
- `unigateway service <SUBCOMMAND>`
  - `list`: list services.
  - `create --id <ID> --name <NAME>`: create/update service.
  - `delete --id <ID> [--force]`: delete service (optionally cascade to keys).
- `unigateway provider <SUBCOMMAND>`
  - `list`: list providers.
  - `add --name <NAME> --type <TYPE> --endpoint-id <ID> [--base-url <URL>] --api-key <KEY> [--model-mapping <JSON>]`: add provider.
  - `delete --id <ID>`: delete provider.
  - `bind --service-id <SID> --provider-id <PID>`: bind service and provider.
- `unigateway api-key <SUBCOMMAND>`
  - `list`: list API keys.
  - `create --name <NAME> [--service-id <SID>] [--provider-id <PID>...] [--quota-limit <N>] [--qps-limit <F>] [--concurrency-limit <N>]`: create key; auto-create service if no `--service-id`; bind given providers to that service.
  - `revoke --key <KEY> [--delete-service]`: revoke key; optionally delete its service.
- `unigateway metrics [--config <PATH>]`
  - Print/export basic metrics (total requests, per-endpoint counts, etc.).

Implement in `cli.rs` with `clap::Parser` + `Subcommand` and match dispatch.

### 3. Output and AI/Script Friendliness

- List/metrics commands: default human-friendly table/text; support `--format json` and `--format plain`.
- Create commands with JSON output should return a consistent structure, e.g.:

```json
{
  "success": true,
  "data": {
    "service_id": "svc-xxx",
    "service_name": "My Service",
    "provider_id": 1,
    "api_key": "sk-...."
  }
}
```

So AI/scripts can parse stdout for automation.

### 4. Relation to HTTP Admin API

- CLI subcommands call `GatewayState` (TOML-based config) directly; no HTTP.
- `/api/admin/...` remains for future Web UI or remote management and for external integration.
- Both CLI and HTTP handlers: parse args → call shared query/mutation → format output.

### 5. Directory and Modules (Current)

- No `src/app/` directory: `src/server.rs` has `run()` and route registration; `AppConfig` in `types.rs`; gateway and admin logic in single-file modules under `src/`.
- `src/cli.rs`: CLI parsing and dispatch; uses `GatewayState` for create/bind/print_metrics.
- No `src/ui/` in this repo; primary management is CLI + `/api/admin/*` JSON API.

### 6. Suggested Next Steps

1. Align `cli.rs` subcommands with this doc (minimal subset first).
2. Add `--format json` for key subcommands and a common JSON response shape.
3. Document CLI-first as the recommended usage in README and project-architecture; mark Web UI as deprecated/optional.
