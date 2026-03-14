# UniGateway Directory Structure

## Current Layout (CLI-first, no UI, flat)

```
unigateway/
├── .github/workflows/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── rust-toolchain.toml
├── docs/
│   ├── directory-structure.md   # this file
│   ├── project-architecture.md
│   ├── usage-scenarios-and-routing-design.md
│   ├── cli-design.md
│   ├── refactor-summary.md
│   ├── admin-refactor-plan.md   # historical; superseded by flat layout
│   └── app-modules-flat.md      # historical; now flat under src/
│
└── src/
    ├── main.rs       # binary entry: clap CLI, subcommand dispatch; no subcommand = run()
    ├── app.rs        # run(config), route registration, pub use storage::hash_password
    ├── types.rs      # AppConfig, AppState, GatewayApiKey, etc.
    ├── gateway.rs    # openai_chat, anthropic_messages
    ├── storage.rs    # init_db, hash_password, gateway queries and rate-limit data
    ├── system.rs     # health, metrics, models
    ├── authz.rs      # is_admin_authorized (x-admin-token)
    ├── dto.rs        # admin API request/response and Row types
    ├── queries.rs    # admin read-only queries
    ├── mutations.rs  # admin writes
    ├── provider.rs   # api_list_providers, api_create_provider, api_bind_provider
    ├── service.rs    # api_list_services, api_create_service
    ├── api_key.rs    # api_list_api_keys, api_create_api_key
    ├── cli.rs        # init_admin, create_service, create_provider, bind_provider, create_api_key, print_metrics_snapshot
    ├── protocol.rs   # OpenAI/Anthropic protocol conversion and upstream calls
    └── sdk.rs        # UniGatewayClient (optional)
```

## Design Notes

- **No lib**: Single binary only; no `src/lib.rs`.
- **No app/bin/ui**: All modules are single files under `src/*.rs`; entry is `main.rs`.
- **Management**: CLI + JSON API only; no Web UI; admin auth via `x-admin-token`.
- **Routes**: Registered in `app.rs` `run()`: `/health`, `/metrics`, `/v1/models`, `/api/admin/*`, `/v1/chat/completions`, `/v1/messages`.

See `project-architecture.md` for architecture details and `refactor-summary.md` for refactor status.
