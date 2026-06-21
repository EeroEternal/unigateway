# `/v1/models` Registry Helpers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class, read-only registry helpers to `unigateway-config` and an OpenAI model-object helper to `unigateway-protocol`, then expose them through `unigateway-sdk`.

**Architecture:** Extend `unigateway-config/src/schema.rs` with `ServiceModel`, `AuthError`, and `routing_ids_for`; implement `GatewayState::list_service_models`, `list_service_model_ids`, and `authorize_readonly` in `unigateway-config/src/admin.rs`; add `openai_model_object` to `unigateway-protocol/src/responses/render.rs`; wire SDK re-export behind a new `config` feature. All changes are additive and follow existing crate boundaries.

**Tech Stack:** Rust, Tokio, serde_json, existing UniGateway workspace crates.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `unigateway-config/src/schema.rs` | New public types: `ServiceModel`, `AuthError`, `routing_ids_for` |
| `unigateway-config/src/admin.rs` | `GatewayState` methods: `list_service_models`, `list_service_model_ids`, `authorize_readonly` and their tests |
| `unigateway-config/src/lib.rs` | Re-export new public types |
| `unigateway-protocol/src/responses/render.rs` | `openai_model_object` helper |
| `unigateway-protocol/src/lib.rs` | Export `openai_model_object` |
| `unigateway-protocol/src/responses/tests.rs` | Unit test for `openai_model_object` |
| `unigateway-sdk/Cargo.toml` | Add `unigateway-config` optional dependency and `config` feature |
| `unigateway-sdk/src/lib.rs` | Feature-gated re-export of `unigateway_config` as `config` |

---

### Task 1: Add `ServiceModel`, `routing_ids_for`, and `AuthError` to `unigateway-config`

**Files:**
- Modify: `unigateway-config/src/schema.rs`

- [ ] **Step 1: Append the new types after `ServiceProvider`**

Insert the following block after the `ServiceProvider` struct definition (around line 242):

```rust
/// A model exposed by a service's bound providers, suitable for OpenAI-compatible `/v1/models`.
#[derive(Debug, Clone)]
pub struct ServiceModel {
    /// Primary routing id in `provider/alias` composite shape.
    pub id: String,
    /// Bare alias (matches `model_mapping` key or `default_model`).
    pub alias: String,
    /// Upstream canonical model name (`model_mapping` value), if different from alias.
    pub canonical: Option<String>,
    /// Provider name that owns this model; maps to OpenAI `owned_by`.
    pub owned_by: String,
}

impl ServiceModel {
    /// Returns all routing-acceptable ids for this model: composite and bare alias.
    pub fn routing_ids(&self) -> Vec<&str> {
        vec![self.id.as_str(), self.alias.as_str()]
    }
}

/// Returns the routing-acceptable id shapes for a provider/alias pair.
///
/// Order: composite `provider/alias` first, then bare `alias`.
pub fn routing_ids_for(provider: &str, alias: &str) -> Vec<String> {
    vec![format!("{}/{}", provider, alias), alias.to_string()]
}

/// Read-only authorization error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Key not found.
    InvalidKey,
    /// Key exists but is inactive.
    InactiveKey,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidKey => write!(f, "invalid api key"),
            AuthError::InactiveKey => write!(f, "inactive api key"),
        }
    }
}

impl std::error::Error for AuthError {}
```

- [ ] **Step 2: Commit**

```bash
git add unigateway-config/src/schema.rs
git commit -m "feat(config): add ServiceModel, routing_ids_for, AuthError"
```

---

### Task 2: Unit-test `routing_ids_for`

**Files:**
- Modify: `unigateway-config/src/schema.rs`

- [ ] **Step 1: Add tests at the bottom of `schema.rs`**

Append inside the existing `#[cfg(test)]` module (create one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::{routing_ids_for, ServiceModel};

    #[test]
    fn routing_ids_for_returns_composite_then_alias() {
        let ids = routing_ids_for("alpha", "gpt-4o");
        assert_eq!(ids, vec!["alpha/gpt-4o", "gpt-4o"]);
    }

    #[test]
    fn service_model_routing_ids_match_routing_ids_for() {
        let model = ServiceModel {
            id: "alpha/gpt-4o".to_string(),
            alias: "gpt-4o".to_string(),
            canonical: Some("gpt-4o-2024-08-06".to_string()),
            owned_by: "alpha".to_string(),
        };
        assert_eq!(model.routing_ids(), vec!["alpha/gpt-4o", "gpt-4o"]);
    }
}
```

- [ ] **Step 2: Run the tests**

```bash
cargo test -p unigateway-config routing_ids_for -- --nocapture
```

Expected: tests pass.

- [ ] **Step 3: Commit**

```bash
git add unigateway-config/src/schema.rs
git commit -m "test(config): routing_ids_for and ServiceModel::routing_ids"
```

---

### Task 3: Implement `GatewayState::list_service_models` and `list_service_model_ids`

**Files:**
- Modify: `unigateway-config/src/admin.rs`

- [ ] **Step 1: Import `ServiceModel`**

Change the `use super::{...}` block at the top of `admin.rs` to include `ServiceModel`:

```rust
use super::{
    ApiKeyEntry, BindingEntry, GatewayConfigFile, GatewayState, ModeView, ProviderEntry,
    ProviderModelOptions, ProviderView, ServiceEntry, ServiceModel, build_mode_views, default_round_robin,
};
```

- [ ] **Step 2: Add `list_service_models` and `list_service_model_ids` inside `impl GatewayState`**

Insert after `pub async fn list_provider_views(&self) -> Vec<ProviderView>` (around line 172):

```rust
    /// Returns the structured model catalog for a service.
    ///
    /// Provider order follows binding priority (ascending). Aliases within a
    /// provider come from `model_mapping` keys (lexicographically sorted) followed
    /// by `default_model` if it is non-empty and not already present.
    pub async fn list_service_models(&self, service_id: &str) -> Vec<ServiceModel> {
        let providers = self.select_all_providers_for_service(service_id, "").await;
        let mut result = Vec::new();
        for provider in providers {
            let mut aliases: Vec<String> = Vec::new();
            let mut mapping = std::collections::BTreeMap::new();

            if let Some(raw) = provider.model_mapping.as_deref() {
                let trimmed = raw.trim();
                if trimmed.starts_with('{') {
                    if let Ok(parsed) =
                        serde_json::from_str::<std::collections::BTreeMap<String, String>>(trimmed)
                    {
                        mapping = parsed;
                    }
                }
            }

            for key in mapping.keys() {
                let alias = key.trim();
                if !alias.is_empty() && !aliases.iter().any(|a| a == alias) {
                    aliases.push(alias.to_string());
                }
            }

            if let Some(default) = provider.default_model.as_deref() {
                let default = default.trim();
                if !default.is_empty() && !aliases.iter().any(|a| a == default) {
                    aliases.push(default.to_string());
                }
            }

            for alias in aliases {
                let canonical = mapping.get(&alias).cloned();
                result.push(ServiceModel {
                    id: format!("{}/{}", provider.name, alias),
                    alias,
                    canonical,
                    owned_by: provider.name.clone(),
                });
            }
        }
        result
    }

    /// Returns a flat, expanded, deduplicated list of `(id, owned_by)` pairs
    /// suitable for emitting an OpenAI-compatible `/v1/models` response.
    ///
    /// Deduplication happens on the final expanded id set (composite + bare alias).
    /// The first occurrence is retained.
    pub async fn list_service_model_ids(&self, service_id: &str) -> Vec<(String, String)> {
        let models = self.list_service_models(service_id).await;
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for model in models {
            for id in model.routing_ids() {
                if seen.insert(id.to_string()) {
                    result.push((id.to_string(), model.owned_by.clone()));
                }
            }
        }
        result
    }
```

- [ ] **Step 3: Commit**

```bash
git add unigateway-config/src/admin.rs
git commit -m "feat(config): list_service_models and list_service_model_ids"
```

---

### Task 4: Implement `GatewayState::authorize_readonly`

**Files:**
- Modify: `unigateway-config/src/admin.rs`

- [ ] **Step 1: Add `authorize_readonly` inside `impl GatewayState`**

Insert after `list_service_model_ids`:

```rust
    /// Validates that an API key exists and is active without consuming quota
    /// or acquiring runtime limits.
    pub async fn authorize_readonly(&self, raw_key: &str) -> Result<GatewayApiKey, AuthError> {
        let key = self
            .find_gateway_api_key(raw_key)
            .await
            .ok_or(AuthError::InvalidKey)?;
        if key.is_active != 1 {
            return Err(AuthError::InactiveKey);
        }
        Ok(key)
    }
```

- [ ] **Step 2: Commit**

```bash
git add unigateway-config/src/admin.rs
git commit -m "feat(config): authorize_readonly"
```

---

### Task 5: Test `list_service_models`, `list_service_model_ids`, and `authorize_readonly`

**Files:**
- Modify: `unigateway-config/src/admin.rs`

- [ ] **Step 1: Append tests to the existing `#[cfg(test)]` module**

Add at the end of the file:

```rust
    #[tokio::test]
    async fn list_service_models_orders_providers_and_aliases() {
        use crate::ProviderModelOptions;

        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("svc", "Service").await;

        let alpha_id = state
            .create_provider_with_models(
                "alpha",
                "openai",
                "moonshot:global",
                None,
                "sk-alpha",
                ProviderModelOptions {
                    default_model: Some("moonshot-v1-8k"),
                    model_mapping: Some("{\"gpt-4\":\"moonshot-v1-8k\"}"),
                },
            )
            .await;
        let beta_id = state
            .create_provider_with_models(
                "beta",
                "openai",
                "moonshot:global",
                None,
                "sk-beta",
                ProviderModelOptions {
                    default_model: Some("moonshot-v1-32k"),
                    model_mapping: Some("{\"gpt-4o\":\"moonshot-v1-32k\"}"),
                },
            )
            .await;

        state
            .bind_provider_to_service_with_priority("svc", beta_id, 5)
            .await
            .expect("bind beta");
        state
            .bind_provider_to_service_with_priority("svc", alpha_id, 10)
            .await
            .expect("bind alpha");

        let models = state.list_service_models("svc").await;
        assert_eq!(models.len(), 3);

        // Provider order: beta (prio 5) before alpha (prio 10).
        assert_eq!(models[0].owned_by, "beta");
        assert_eq!(models[0].id, "beta/gpt-4o");
        assert_eq!(models[0].alias, "gpt-4o");
        assert_eq!(models[0].canonical.as_deref(), Some("moonshot-v1-32k"));

        assert_eq!(models[1].owned_by, "beta");
        assert_eq!(models[1].id, "beta/moonshot-v1-32k");
        assert_eq!(models[1].alias, "moonshot-v1-32k");
        assert_eq!(models[1].canonical, None);

        assert_eq!(models[2].owned_by, "alpha");
        assert_eq!(models[2].id, "alpha/gpt-4");
    }

    #[tokio::test]
    async fn list_service_models_keeps_default_model_when_mapping_malformed() {
        use crate::ProviderModelOptions;

        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("svc", "Service").await;
        let provider_id = state
            .create_provider_with_models(
                "alpha",
                "openai",
                "moonshot:global",
                None,
                "sk-alpha",
                ProviderModelOptions {
                    default_model: Some("moonshot-v1-8k"),
                    model_mapping: Some("not-json"),
                },
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let models = state.list_service_models("svc").await;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].alias, "moonshot-v1-8k");
        assert_eq!(models[0].canonical, None);
    }

    #[tokio::test]
    async fn list_service_model_ids_dedupes_expanded_ids() {
        use crate::ProviderModelOptions;

        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("svc", "Service").await;

        let alpha_id = state
            .create_provider_with_models(
                "alpha",
                "openai",
                "moonshot:global",
                None,
                "sk-alpha",
                ProviderModelOptions {
                    default_model: None,
                    model_mapping: Some("{\"gpt-4\":\"a-gpt-4\"}"),
                },
            )
            .await;
        let beta_id = state
            .create_provider_with_models(
                "beta",
                "openai",
                "moonshot:global",
                None,
                "sk-beta",
                ProviderModelOptions {
                    default_model: None,
                    model_mapping: Some("{\"gpt-4\":\"b-gpt-4\"}"),
                },
            )
            .await;

        state
            .bind_provider_to_service("svc", alpha_id)
            .await
            .expect("bind alpha");
        state
            .bind_provider_to_service("svc", beta_id)
            .await
            .expect("bind beta");

        let ids = state.list_service_model_ids("svc").await;
        let id_values: Vec<&str> = ids.iter().map(|(id, _)| id.as_str()).collect();

        // Composite ids are unique; bare "gpt-4" appears only once (from alpha).
        assert_eq!(
            id_values,
            vec!["alpha/gpt-4", "gpt-4", "beta/gpt-4"]
        );
        assert_eq!(ids[1].1, "alpha");
    }

    #[tokio::test]
    async fn authorize_readonly_does_not_consume_quota() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("svc", "Service").await;
        state
            .create_api_key("ugk_test_key", "svc", Some(100), None, None)
            .await;

        let before = state.find_gateway_api_key("ugk_test_key").await.unwrap().used_quota;

        let key = state.authorize_readonly("ugk_test_key").await;
        assert!(key.is_ok());

        let after = state.find_gateway_api_key("ugk_test_key").await.unwrap().used_quota;
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn authorize_readonly_rejects_inactive_key() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("svc", "Service").await;
        state
            .create_api_key("ugk_inactive", "svc", None, None, None)
            .await;
        {
            let mut guard = state.write_config().await;
            let key = guard
                .file
                .api_keys
                .iter_mut()
                .find(|k| k.key == "ugk_inactive")
                .expect("key exists");
            key.is_active = false;
            guard.dirty = false;
        }

        let result = state.authorize_readonly("ugk_inactive").await;
        assert_eq!(result, Err(crate::AuthError::InactiveKey));
    }

    #[tokio::test]
    async fn authorize_readonly_rejects_missing_key() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        let result = state.authorize_readonly("ugk_missing").await;
        assert_eq!(result, Err(crate::AuthError::InvalidKey));
    }
```

- [ ] **Step 2: Run the new config tests**

```bash
cargo test -p unigateway-config list_service_model -- --nocapture
cargo test -p unigateway-config authorize_readonly -- --nocapture
```

Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add unigateway-config/src/admin.rs
git commit -m "test(config): list_service_models, list_service_model_ids, authorize_readonly"
```

---

### Task 6: Export new public types from `unigateway-config`

**Files:**
- Modify: `unigateway-config/src/lib.rs`

- [ ] **Step 1: Update the `pub use self::schema` block**

Change:

```rust
pub use self::schema::{
    ApiKeyEntry, BindingEntry, GatewayApiKey, GatewayConfigFile, ModeKey, ModeProvider, ModeView,
    ProviderEntry, ProviderModelOptions, ProviderView, ServiceEntry, build_mode_views,
};
```

To:

```rust
pub use self::schema::{
    ApiKeyEntry, AuthError, BindingEntry, GatewayApiKey, GatewayConfigFile, ModeKey, ModeProvider,
    ModeView, ProviderEntry, ProviderModelOptions, ProviderView, ServiceEntry, ServiceModel,
    build_mode_views,
};
pub use self::schema::routing_ids_for;
```

- [ ] **Step 2: Run a compile check**

```bash
cargo check -p unigateway-config
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add unigateway-config/src/lib.rs
git commit -m "feat(config): export ServiceModel, AuthError, routing_ids_for"
```

---

### Task 7: Add `openai_model_object` helper to `unigateway-protocol`

**Files:**
- Modify: `unigateway-protocol/src/responses/render.rs`
- Modify: `unigateway-protocol/src/lib.rs`

- [ ] **Step 1: Add the helper at the end of `render.rs`**

Append after the existing renderer functions:

```rust
/// Builds an OpenAI-compatible `model` object.
///
/// Output shape:
/// ```json
/// {
///   "id": "...",
///   "object": "model",
///   "created": 0,
///   "owned_by": "..."
/// }
/// ```
pub fn openai_model_object(id: &str, owned_by: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "object": "model",
        "created": 0,
        "owned_by": owned_by,
    })
}
```

- [ ] **Step 2: Export from `unigateway-protocol/src/lib.rs`**

Add `openai_model_object` to the `pub use responses::{...}` block:

```rust
pub use responses::{
    AnthropicStreamAggregator, openai_model_object, render_anthropic_chat_session,
    render_openai_chat_session, render_openai_embeddings_response,
    render_openai_responses_session, render_openai_responses_stream_from_completed,
};
```

- [ ] **Step 3: Commit**

```bash
git add unigateway-protocol/src/responses/render.rs unigateway-protocol/src/lib.rs
git commit -m "feat(protocol): add openai_model_object helper"
```

---

### Task 8: Test `openai_model_object`

**Files:**
- Modify: `unigateway-protocol/src/responses/tests.rs`

- [ ] **Step 1: Add a test**

Append inside the existing `#[cfg(test)]` module:

```rust
#[test]
fn openai_model_object_has_expected_shape() {
    let value = openai_model_object("alpha/gpt-4o", "alpha");
    assert_eq!(value["id"], "alpha/gpt-4o");
    assert_eq!(value["object"], "model");
    assert_eq!(value["created"], 0);
    assert_eq!(value["owned_by"], "alpha");
}
```

- [ ] **Step 2: Run the test**

```bash
cargo test -p unigateway-protocol openai_model_object -- --nocapture
```

Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add unigateway-protocol/src/responses/tests.rs
git commit -m "test(protocol): openai_model_object shape"
```

---

### Task 9: Wire `unigateway-config` into `unigateway-sdk`

**Files:**
- Modify: `unigateway-sdk/Cargo.toml`
- Modify: `unigateway-sdk/src/lib.rs`

- [ ] **Step 1: Update `unigateway-sdk/Cargo.toml`**

Add the `config` feature and optional dependency:

```toml
[features]
default = ["host"]
embed = ["host"]
config = ["dep:unigateway-config"]
core = ["dep:unigateway-core"]
protocol = ["core", "dep:unigateway-protocol"]
host = ["protocol", "dep:unigateway-host"]
testing = ["host", "unigateway-host/testing"]
```

Add to `[dependencies]`:

```toml
unigateway-config = { workspace = true, optional = true }
```

- [ ] **Step 2: Re-export in `unigateway-sdk/src/lib.rs`**

Add at the end of the file:

```rust
#[cfg(feature = "config")]
pub use unigateway_config as config;
```

- [ ] **Step 3: Commit**

```bash
git add unigateway-sdk/Cargo.toml unigateway-sdk/src/lib.rs
git commit -m "feat(sdk): add optional unigateway-config re-export"
```

---

### Task 10: Verify workspace build and tests

- [ ] **Step 1: Format**

```bash
cargo fmt --all
```

- [ ] **Step 2: Clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: no warnings.

- [ ] **Step 3: Tests**

```bash
cargo test --workspace
```

Expected: all tests pass.

- [ ] **Step 4: Final commit (if formatting produced changes)**

```bash
git add -A
git commit -m "chore: fmt"
```

---

## Spec Coverage Check

| Spec Requirement | Task |
| --- | --- |
| P0: `ServiceModel` + `list_service_models` | Task 1, Task 3 |
| P1: `routing_ids_for` / `ServiceModel::routing_ids` | Task 1, Task 2 |
| P2: deterministic ordering & expanded-id dedup | Task 3, Task 5 |
| P3: `authorize_readonly` with zero quota consumption | Task 4, Task 5 |
| P4: `openai_model_object` | Task 7, Task 8 |
| SDK `config` namespace re-export | Task 9 |

## Placeholder Scan

- No TBD/TODO/fill-in-details remain.
- Every code step contains the actual code to write.
- Every test step contains the actual test code.
- Every command is exact.

## Type Consistency Check

- `ServiceModel` fields and `routing_ids` signature match across Task 1, Task 3, Task 5.
- `AuthError` variants match across Task 1, Task 4, Task 5.
- `GatewayState` method signatures match the spec and are used consistently in tests.
- `openai_model_object` signature and output shape match the spec in Task 7 and Task 8.
