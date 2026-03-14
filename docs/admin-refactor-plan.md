# Admin Module Refactor Plan

> **Note**: This is a historical planning doc. The project has since adopted “remove UI + flatten src”: no `app/` directory; management is JSON API + CLI only. See `refactor-summary.md` and `directory-structure.md`.

## Goal

At the time, `src/app/admin.rs` had too many responsibilities: page routes, admin API, auth, SQL, HTML concatenation, partial responses, response types. The P1 goal was to split it into a maintainable structure without adding features.

## Problems

1. **Single large file**: dashboard, providers, api keys, services, logs, settings, detail pages, partials, headless API, metrics/models, DTOs, delete logic, auth—all in one place; hard to change one thing without touching others; UI, API, and data access coupled.
2. **SQL and HTML in handlers**: Handlers did login check, SQL, shaping, HTML; poor reuse and testability.
3. **Mixed UI and API**: Browser pages, HTMX partials, and JSON admin API in the same file.

## P1 Suggested Split (Historical)

Split `src/app/admin.rs` into:

- `admin/mod.rs`, `authz.rs`, `dto.rs`, `pages.rs`, `partials.rs`, `api.rs`, `queries.rs`, `mutations.rs`, `render.rs`

Or a smaller first step: `mod.rs`, `pages.rs`, `api.rs`, `data.rs`, `authz.rs`.

**Module roles**: mod.rs re-exports; authz for `is_admin_authorized`; dto for request/response/Row; pages for page handlers; partials for HTMX; api for JSON admin + health/metrics/models; queries for read SQL; mutations for writes; render for HTML helpers.

**Order**: (1) Physical split only—no behavior change; (2) Extract queries and mutations; (3) Optionally extract render. Keep `admin::xxx` in app.rs so routing stays stable.

## What to Avoid in P1

Do not change copy, SQL, service/key model, or gateway logic; do not introduce a template engine. Goal: structure first, then behavior.

## Outcome (Historical)

The large admin.rs was split into authz, dto, api, pages, partials, then queries, mutations, render. Handlers became thinner; SQL and HTML centralized. That layout was later superseded by removing UI and flattening under `src/` (current state).
