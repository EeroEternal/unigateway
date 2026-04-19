# unigateway-host

Reusable host contracts and dispatch helpers for embedders building on top of UniGateway.

Version compatibility:

- Keep `unigateway-host`, `unigateway-core`, and `unigateway-protocol` on the same minor version.
- When in doubt, pin all three crates to the exact same release instead of mixing independently resolved `^1.x` ranges.

The main host contract lives in `unigateway-host::host` and only requires service-pool lookup through `PoolHost`.
Env-backed fallback helpers live separately under `unigateway-host::env` so embedders without env fallback do not need to implement extra glue.

Testing helpers:

- Enable the `testing` feature to use `unigateway_host::testing::MockHost` and `unigateway_host::testing::build_context` in embedder integration tests.