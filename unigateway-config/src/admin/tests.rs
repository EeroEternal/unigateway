//! Integration tests for the split admin operation modules.

#[cfg(test)]
mod suite {
    use crate::GatewayState;
    use std::path::Path;
    use tempfile::tempdir;

    #[tokio::test]
    async fn list_mode_views_reflects_default_and_bindings() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("fast", "Fast").await;
        state.create_service("strong", "Strong").await;
        let provider_id = state
            .create_provider(
                "deepseek-main",
                "openai",
                "deepseek:global",
                Some("https://api.deepseek.com"),
                "sk-provider",
                None,
            )
            .await;
        state
            .bind_provider_to_service_with_priority("fast", provider_id, 10)
            .await
            .expect("bind provider");
        state
            .set_default_mode("fast")
            .await
            .expect("set default mode");

        let modes = state.list_mode_views().await;
        let fast = modes
            .iter()
            .find(|mode| mode.id == "fast")
            .expect("fast mode present");
        let strong = modes
            .iter()
            .find(|mode| mode.id == "strong")
            .expect("strong mode present");

        assert!(fast.is_default);
        assert!(!strong.is_default);
        assert_eq!(fast.providers.len(), 1);
        assert_eq!(fast.providers[0].name, "deepseek-main");
    }

    #[tokio::test]
    async fn rebind_api_key_service_preserves_limits_and_usage() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("fast", "Fast").await;
        state.create_service("strong", "Strong").await;
        state
            .create_api_key("ugk_test_key", "fast", Some(100), Some(2.5), Some(3))
            .await;

        {
            let mut guard = state.write_config().await;
            let key = guard
                .file
                .api_keys
                .iter_mut()
                .find(|item| item.key == "ugk_test_key")
                .expect("key exists");
            key.used_quota = 37;
            key.is_active = false;
            guard.dirty = false;
        }

        state
            .rebind_api_key_service("ugk_test_key", "strong")
            .await
            .expect("rebind key");

        let keys = state.list_api_keys().await;
        let key = keys
            .iter()
            .find(|item| item.key == "ugk_test_key")
            .expect("key exists");

        assert_eq!(key.service_id, "strong");
        assert_eq!(key.used_quota, 37);
        assert_eq!(key.quota_limit, Some(100));
        assert_eq!(key.qps_limit, Some(2.5));
        assert_eq!(key.concurrency_limit, Some(3));
        assert!(!key.is_active);
    }

    #[tokio::test]
    async fn rebind_api_key_service_rejects_unknown_inputs() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let state = GatewayState::load(Path::new(&config_path))
            .await
            .expect("load state");

        state.create_service("fast", "Fast").await;
        state
            .create_api_key("ugk_test_key", "fast", None, None, None)
            .await;

        let missing_service = state
            .rebind_api_key_service("ugk_test_key", "missing")
            .await
            .expect_err("missing service should fail");
        assert!(
            missing_service
                .to_string()
                .contains("service 'missing' not found")
        );

        let missing_key = state
            .rebind_api_key_service("ugk_missing", "fast")
            .await
            .expect_err("missing key should fail");
        assert!(
            missing_key
                .to_string()
                .contains("api key 'ugk_missing' not found")
        );
    }

    #[tokio::test]
    async fn provider_views_update_and_delete_by_name() {
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
                    metadata: None,
                },
            )
            .await;
        let _beta_id = state
            .create_provider_with_models(
                "beta",
                "anthropic",
                "",
                Some("https://api.anthropic.com"),
                "sk-beta",
                ProviderModelOptions {
                    default_model: None,
                    model_mapping: None,
                    metadata: None,
                },
            )
            .await;

        state
            .bind_provider_to_service("svc", alpha_id)
            .await
            .expect("bind provider");

        let views = state.list_provider_views().await;
        assert_eq!(views.len(), 2);

        let alpha = views
            .iter()
            .find(|v| v.name == "alpha")
            .expect("alpha view present");
        assert_eq!(alpha.id, 0);
        assert_eq!(alpha.provider_type, "openai");
        assert_eq!(alpha.endpoint_id.as_deref(), Some("moonshot:global"));
        assert_eq!(alpha.base_url, None);
        assert_eq!(alpha.default_model.as_deref(), Some("moonshot-v1-8k"));
        assert_eq!(alpha.models, vec!["gpt-4"]);
        assert!(alpha.is_enabled);

        let beta = views
            .iter()
            .find(|v| v.name == "beta")
            .expect("beta view present");
        assert_eq!(beta.id, 1);
        assert_eq!(beta.provider_type, "anthropic");
        assert_eq!(beta.endpoint_id, None);
        assert_eq!(beta.base_url.as_deref(), Some("https://api.anthropic.com/"));
        assert_eq!(beta.default_model, None);
        assert!(beta.models.is_empty());

        state
            .update_provider_by_name("alpha", None, None, Some("moonshot-v1-32k"), None)
            .await
            .expect("update provider");

        let views = state.list_provider_views().await;
        let alpha = views
            .iter()
            .find(|v| v.name == "alpha")
            .expect("alpha view present");
        assert_eq!(alpha.default_model.as_deref(), Some("moonshot-v1-32k"));

        state
            .delete_provider_by_name("alpha")
            .await
            .expect("delete provider");

        let views = state.list_provider_views().await;
        assert_eq!(views.len(), 1);
        assert!(views.iter().all(|v| v.name != "alpha"));

        let file = state.config_snapshot().await;
        assert!(file.bindings.iter().all(|b| b.provider_name != "alpha"));

        let missing_update = state
            .update_provider_by_name("missing", None, None, Some("x"), None)
            .await
            .expect_err("update missing provider should fail");
        assert!(
            missing_update
                .to_string()
                .contains("provider 'missing' not found")
        );

        let missing_delete = state
            .delete_provider_by_name("missing")
            .await
            .expect_err("delete missing provider should fail");
        assert!(
            missing_delete
                .to_string()
                .contains("provider 'missing' not found")
        );
    }

    #[tokio::test]
    async fn list_service_models_trims_mapping_keys_and_sorts_aliases() {
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
                    model_mapping: Some(r#"{" zzz ":"z-model"," aaa ":"a-model"}"#),
                    metadata: None,
                },
            )
            .await;
        state
            .bind_provider_to_service("svc", provider_id)
            .await
            .expect("bind provider");

        let models = state.list_service_models("svc").await;
        assert_eq!(models.len(), 3);

        // Aliases are trimmed and sorted lexicographically; default_model appended last.
        assert_eq!(models[0].alias, "aaa");
        assert_eq!(models[0].canonical.as_deref(), Some("a-model"));
        assert_eq!(models[1].alias, "zzz");
        assert_eq!(models[1].canonical.as_deref(), Some("z-model"));
        assert_eq!(models[2].alias, "moonshot-v1-8k");
        assert_eq!(models[2].canonical, None);

        // Flattened ids are deduplicated across expanded shapes.
        let ids = state.list_service_model_ids("svc").await;
        let id_values: Vec<&str> = ids.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            id_values,
            vec![
                "alpha/aaa",
                "aaa",
                "alpha/zzz",
                "zzz",
                "alpha/moonshot-v1-8k",
                "moonshot-v1-8k"
            ]
        );
    }

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
                    metadata: None,
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
                    metadata: None,
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
        assert_eq!(models.len(), 4);

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
        assert_eq!(models[2].alias, "gpt-4");
        assert_eq!(models[2].canonical.as_deref(), Some("moonshot-v1-8k"));

        assert_eq!(models[3].owned_by, "alpha");
        assert_eq!(models[3].id, "alpha/moonshot-v1-8k");
        assert_eq!(models[3].alias, "moonshot-v1-8k");
        assert_eq!(models[3].canonical, None);
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
                    metadata: None,
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
                    metadata: None,
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
                    metadata: None,
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
        assert_eq!(id_values, vec!["alpha/gpt-4", "gpt-4", "beta/gpt-4"]);
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

        let before = state
            .find_gateway_api_key("ugk_test_key")
            .await
            .unwrap()
            .used_quota;

        let key = state.authorize_readonly("ugk_test_key").await;
        assert!(key.is_ok());

        let after = state
            .find_gateway_api_key("ugk_test_key")
            .await
            .unwrap()
            .used_quota;
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
}
