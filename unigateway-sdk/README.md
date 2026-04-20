# unigateway-sdk

Thin facade crate for embedders that want a single UniGateway dependency.

Design scope:

- Re-export `unigateway-core`, `unigateway-protocol`, and `unigateway-host` under a single crate.
- Keep feature selection and version alignment in one place.
- Avoid adding a second abstraction layer on top of the underlying crates.

Version compatibility:

- Recommended path: depend on `unigateway-sdk` only.
- If you must mix `unigateway-sdk` with direct `unigateway-core` / `unigateway-protocol` /
	`unigateway-host` dependencies, keep all of them on the same release line.

Feature layout:

- `default`: canonical full embedder stack, implemented by enabling `host`.
- `core`: only `unigateway-core`.
- `protocol`: `unigateway-core` + `unigateway-protocol`.
- `host`: canonical named full stack (`core` + `protocol` + `host`).
- `embed`: compatibility alias for `host` in the 1.x line.
- `testing`: forwards `unigateway-host/testing` so facade-only embedders can reuse host test fixtures.

Example:

```toml
[dependencies]
unigateway-sdk = "1.6"
```

If you disable default features, prefer `features = ["host"]`. `embed` is still accepted as a
1.x compatibility alias.

```rust
use unigateway_sdk::core::UniGatewayEngine;
use unigateway_sdk::host as ug_host;
use ug_host::{HostContext, PoolHost};
use unigateway_sdk::protocol::openai_payload_to_chat_request;
```

Positioning:

- Use this crate when you want one stable dependency entry point for embedding.
- Reach through to the underlying crates when you need real functionality.
- Do not expect `unigateway-sdk` to grow a separate state model or execution layer.

That means examples should still look like normal `unigateway-core` / `unigateway-protocol` /
`unigateway-host` usage, just under the `unigateway_sdk::...` namespace.

Minimal endpoint example:

```rust
use std::collections::HashMap;

use unigateway_sdk::core::{Endpoint, ModelPolicy, ProviderKind, SecretString};

let endpoint = Endpoint {
	endpoint_id: "ep-openai-main".to_string(),
	// Used by provider-hint matching and operator-facing display.
	provider_name: Some("openai-main".to_string()),
	// Preserves the original upstream/source identifier for hint matching.
	source_endpoint_id: Some("openai-main".to_string()),
	// Enables family-level hints such as "openai" or "deepseek".
	provider_family: Some("openai".to_string()),
	provider_kind: ProviderKind::OpenAiCompatible,
	driver_id: "openai-compatible".to_string(),
	base_url: "https://api.openai.com".to_string(),
	api_key: SecretString::new("sk-..."),
	model_policy: ModelPolicy::default(),
	enabled: true,
	metadata: HashMap::new(),
};
```

Field guidance:

- `provider_name`: stable operator-facing label used by hint matching.
- `source_endpoint_id`: original upstream or datastore id preserved for stable matching.
- `provider_family`: vendor-family grouping used by broader hints such as `openai` or `deepseek`.