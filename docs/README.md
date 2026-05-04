# UniGateway Documentation Index

This repository is a Rust library workspace. It does not ship an in-tree HTTP server or CLI binary. Documentation is organized by audience and purpose:

- `design`: architecture, protocol conversion, and library-level design notes.
- `guide`: configuration and embedder-facing usage guides.
- `dev`: contributor notes, roadmap documents, and historical refactor context.

## Design

| File | Description |
| --- | --- |
| [`arch.md`](design/arch.md) | Current library layering, config-to-core projection, and host request flow. |
| [`protocol-conversion.md`](design/protocol-conversion.md) | Protocol conversion architecture, neutral chat model, OpenAI/Anthropic request and response mapping, and loss rules. |
| [`admin.md`](design/admin.md) | `/api/admin/*` JSON contracts for embedders that build their own management gateway or UI. |
| [`queue.md`](design/queue.md) | Concurrency queueing and backpressure design. Runtime helpers live in `unigateway-config::runtime`; HTTP integration belongs to embedders. |
| [`scheduling.md`](design/scheduling.md) | Longer-term scheduling and queueing direction. |
| [`cli.md`](design/cli.md) | Deprecated historical CLI product draft; management surfaces now belong to embedder applications. |

## Guide

| File | Description |
| --- | --- |
| [`config.md`](guide/config.md) | `unigateway.toml` fields and rules for syncing config state into core pools. |
| [`providers.md`](guide/providers.md) | TOML and call examples for common providers. |
| [`embed.md`](guide/embed.md) | Embedding UniGateway in another Rust application. |
| [`embedder_patterns.md`](guide/embedder_patterns.md) | Production embedding patterns: dynamic state awareness, external routing, `GatewayHooks` extension, and runtime refresh. |

## Dev

| File | Description |
| --- | --- |
| [`memory.md`](dev/memory.md) | Fast mental model and code entry points for contributors and AI agents. |
| [`embed-sdk.md`](dev/embed-sdk.md) | `unigateway-sdk` facade positioning and public API evolution. |
| [`public-api-typing.md`](dev/public-api-typing.md) | Stepwise plan for stronger public request API typing. |
| [`roadmap.md`](dev/roadmap.md) | Project phases and priorities. |
| [`refactor-baseline.md`](dev/refactor-baseline.md) | Historical split notes and structure debt; the root `src/` product shell has been removed. |
| [`local-gateway.md`](dev/local-gateway.md) | Historical and exploratory local gateway notes; implementation belongs to embedders. |
| [`openclaw.md`](dev/openclaw.md) | Example flow for OpenClaw integration against a compatible HTTP gateway. |

Repository-wide agent collaboration conventions live in [`../AGENTS.md`](../AGENTS.md).
