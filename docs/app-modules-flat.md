# App Directory Flattened by Function

> **Note**: This doc describes an intermediate “app/ by feature” layout. The codebase has since moved to a **fully flat layout under src/**: no `app/` directory, no UI (dashboard, logs, settings, auth, shell, render removed); entry is `main.rs`. See `refactor-summary.md` and `directory-structure.md`.

## Goal

Replace a single broad `admin` directory with a flat layout under `app/` by **domain**: one area per feature (service, provider, api_key, dashboard, logs, settings), plus shared layers (authz, dto, queries, mutations, render, shell) and system (system).

## Proposed Structure (Historical)

```
src/app/
├── mod.rs              # entry: run, AppConfig, route registration
├── auth.rs             # login/logout
├── gateway.rs          # gateway chat entry
├── storage.rs          # DB init and gateway queries
├── types.rs            # AppState, GatewayApiKey, etc.
├── authz.rs            # admin auth (from admin/authz)
├── dto.rs              # admin request/response/Row (from admin/dto)
├── shell.rs            # layout and login helpers (from admin/shell)
├── render.rs           # HTML fragment rendering (from admin/render)
├── queries.rs          # admin read queries
├── mutations.rs        # admin writes
├── dashboard.rs        # home, dashboard, stats partial
├── provider.rs         # provider list/detail/create/delete (pages + partials + JSON API)
├── service.rs         # service list/detail/delete
├── api_key.rs          # API key list/detail/create/delete
├── logs.rs             # request logs page and list partial
├── settings.rs         # settings page
└── system.rs           # health, metrics, models (no UI)
```

## Module Roles (Historical)

| Module     | Role |
|-----------|------|
| dashboard | home, admin_page, admin_dashboard, admin_stats_partial |
| provider  | admin_providers, detail page, list partial, create partial, delete; api_list_providers, api_create_provider, api_bind_provider |
| service   | admin_services page, detail, list partial, delete; api_list_services, api_create_service |
| api_key   | admin_api_keys page, detail, list partial, create partial, delete; api_list_api_keys, api_create_api_key |
| logs      | admin_logs page, list partial |
| settings  | admin_settings_page |
| system    | health, metrics, models |

Routes were registered in `app/mod.rs`; former `admin::xxx` became `dashboard::xxx`, `provider::xxx`, etc.
