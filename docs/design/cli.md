# CLI design (historical)

The in-repository **`ug` CLI and `unigateway-cli` crate have been removed**. This file previously sketched a CLI-first product; that surface belongs in a **separate host application** (for example your management gateway) if you still want terminal UX.

For embedding the engine and config stack, use **`unigateway-sdk`** and [`../guide/embed.md`](../guide/embed.md). For programmatic config mutation shapes that a CLI or admin UI might call, see [`admin.md`](./admin.md) and `unigateway-config::admin`.

If you need the old CLI implementation as a reference, check **git history** before the SDK-only pivot.
