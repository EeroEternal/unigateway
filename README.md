<div align="center">
  <h1>UniGateway</h1>
  <p>
    <strong>Rust SDK for LLM routing, protocol translation, and provider execution.</strong>
  </p>
  <p>
    Embed a local-first engine behind your own HTTP gateway, desktop app, or agent host.
  </p>
  <p>
    🌐 <strong><a href="http://unigate.sh/">Website: unigate.sh</a></strong>
  </p>
  <p>
    <a href="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml"><img src="https://github.com/EeroEternal/unigateway/actions/workflows/rust.yml/badge.svg" alt="Build Status"></a>
    <a href="https://crates.io/crates/unigateway-sdk"><img src="https://img.shields.io/crates/v/unigateway-sdk.svg" alt="Crate"></a>
    <a href="https://github.com/EeroEternal/unigateway/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

This repository is a **library workspace** (no bundled CLI or HTTP server). Use it from your application—for example a dedicated management gateway—to unify OpenAI- and Anthropic-shaped traffic across multiple upstream providers with modes, bindings, and failover handled in `unigateway-core`.

## Dependency

Recommended path: depend on **`unigateway-sdk`** only. It re-exports `unigateway_sdk::core`, `unigateway_sdk::protocol`, and `unigateway_sdk::host` without a second abstraction layer.

```toml
[dependencies]
unigateway-sdk = "1.6"
```

If you mix `unigateway-sdk` with direct `unigateway-core` / `unigateway-protocol` / `unigateway-host` crates, keep them on the **same release line**.

## Docs

- [docs/guide/embed.md](docs/guide/embed.md) — integration guide for embedders
- [docs/dev/embed-sdk.md](docs/dev/embed-sdk.md) — facade positioning and API evolution
- [docs/design/arch.md](docs/design/arch.md) — architecture
- [docs/README.md](docs/README.md) — full doc index

## Build

```bash
cargo build --workspace
cargo test --workspace
```

## Agent / contributor notes

See [`AGENTS.md`](AGENTS.md) for conventions (build, docs layout, safety).

## License

MIT. See [LICENSE](LICENSE).
