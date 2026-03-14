# UniGateway Refactor Goals and Current Status

## I. Achieved Goals

### 1. Minimal, CLI-first

- **Single binary**: Removed `lib.rs` and Cargo `[lib]`; the project is a single executable crate with no library.
- **Management**: CLI subcommands and JSON API (`/api/admin/*`) only; no Web UI.
- **Auth**: Admin API uses `x-admin-token` header (or allow when unset); Cookie-based login and sessions removed.

### 2. Web UI Removed

- **Removed**: Entire `src/ui/` (e.g. `mod.rs`, `templates.rs`) and all UI-related `admin_*` page/partial handlers.
- **Kept**: JSON admin API and gateway routes only:
  - `/health`, `/metrics`, `/v1/models`
  - `/api/admin/services`, `/api/admin/providers`, `/api/admin/bindings`, `/api/admin/api-keys`
  - `/v1/chat/completions`, `/v1/messages`
- **Removed modules**: auth, dashboard, logs, settings, shell, render (UI-only).

### 3. Flat Directory Layout

- **Entry**: Binary entry is `src/main.rs` (Cargo.toml `[[bin]] path = "src/main.rs"`).
- **No app/bin/ui**: Removed `src/app/`, `src/bin/`, `src/ui/`; all modules live under `src/`.
- **Current src layout**:
  - `main.rs`: CLI (clap), subcommand dispatch; default = start gateway.
  - `app.rs`: Thin layer: `run(config)` and route registration (no UI routes); re-exports `storage::hash_password`.
  - `types.rs`: `AppConfig` (with `from_env()`), `AppState`, and gateway types.
  - `gateway.rs`, `storage.rs`, `dto.rs`, `queries.rs`, `mutations.rs`, `authz.rs`, `provider.rs`, `service.rs`, `api_key.rs`, `system.rs`: Gateway and admin logic.
  - `cli.rs`, `protocol.rs`, `sdk.rs`: CLI implementation, protocol adapters, optional SDK.

### 4. CLI Subcommands (Current)

- **Serve**: `--bind`, `--db`, `--no-ui` (no UI; kept for compatibility).
- **InitAdmin**: Initialize/update admin user password.
- **Metrics**: Print metrics snapshot from DB.
- **CreateService** / **CreateProvider** / **BindProvider** / **CreateApiKey**: Corresponding admin operations.

Scenarios and routing design: see `usage-scenarios-and-routing-design.md`. Future work: quickstart, nested subcommands (e.g. `service list`).

## II. Docs vs Code

- **Architecture and layout**: This doc plus `directory-structure.md` and `project-architecture.md`.
- **CLI target**: `cli-design.md` describes the target CLI (quickstart, `--format json`, etc.); current implementation is a subset.
- **Scenarios and routing**: `usage-scenarios-and-routing-design.md` still applies; management entry is CLI + JSON API only.
