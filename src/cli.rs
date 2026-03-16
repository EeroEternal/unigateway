mod diagnostics;
pub mod guide;
pub mod modes;
pub mod process;
pub mod render;

use anyhow::Result;
use std::path::Path;

use crate::config::GatewayState;

#[cfg(test)]
pub(crate) use diagnostics::summarize_response_text;
pub use diagnostics::{doctor, test_mode};
#[cfg(test)]
pub(crate) use modes::{
    ModeKey, ModeProvider, ModeView, effective_default_mode_id, user_bind_address,
};
pub use modes::{list_modes, show_mode, use_mode};
pub use process::{daemonize, is_running, status_server, stop_server, view_logs};
#[cfg(test)]
pub(crate) use guide::planned_modes;
pub use guide::{
    GuideParams, bind_provider, create_api_key, create_provider, create_service, guide,
    interactive_create_api_key, interactive_create_provider, interactive_create_service,
};
#[cfg(test)]
pub(crate) use render::{
    integrations::{IntegrationTool, parse_integration_tool, render_integration_output_for_tool},
    routes::render_route_explanation,
};
pub use render::{
    integrations::{print_integrations, print_integrations_with_key},
    routes::explain_route,
};

pub async fn config_get(config_path: &str, key: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let value = state.get_config_value(key).await?;
    println!("{}", value);
    Ok(())
}

pub async fn config_set(config_path: &str, key: &str, value: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.set_config_value(key, value).await?;
    state.persist_if_dirty().await?;
    println!("✅ Set '{}' to '{}'", key, value);
    Ok(())
}

pub async fn print_metrics_snapshot(config_path: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let (total, openai_total, anthropic_total, embeddings_total) = state.metrics_snapshot().await;
    println!("unigateway_requests_total {}", total);
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}",
        openai_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}",
        anthropic_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/embeddings\"}} {}",
        embeddings_total
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        GuideParams, IntegrationTool, ModeKey, ModeProvider, ModeView, effective_default_mode_id,
        guide, parse_integration_tool, planned_modes, render_integration_output_for_tool,
        render_route_explanation, summarize_response_text, user_bind_address,
    };
    use crate::config::GatewayState;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn user_bind_address_rewrites_wildcard_host() {
        assert_eq!(user_bind_address("0.0.0.0:3210"), "127.0.0.1:3210");
        assert_eq!(user_bind_address("[::]:3210"), "127.0.0.1:3210");
    }

    #[test]
    fn user_bind_address_keeps_explicit_host() {
        assert_eq!(user_bind_address("127.0.0.1:3210"), "127.0.0.1:3210");
    }

    #[test]
    fn planned_modes_defaults_to_single_mode() {
        let modes = planned_modes(None, None, None, None);
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].0, "default");
    }

    #[test]
    fn effective_default_mode_prefers_explicit_default() {
        let modes = vec![
            ModeView {
                id: "fast".to_string(),
                name: "Fast".to_string(),
                is_default: false,
                routing_strategy: "round_robin".to_string(),
                providers: vec![],
                keys: vec![],
            },
            ModeView {
                id: "strong".to_string(),
                name: "Strong".to_string(),
                is_default: true,
                routing_strategy: "round_robin".to_string(),
                providers: vec![],
                keys: vec![],
            },
        ];

        assert_eq!(effective_default_mode_id(&modes), Some("strong"));
    }

    #[tokio::test]
    async fn guide_creates_single_mode_when_mode_not_specified() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let config_path_str = config_path.to_str().expect("utf8 path");

        let result = guide(
            config_path_str,
            GuideParams {
                service_id: None,
                service_name: None,
                provider_name: "deepseek-main",
                provider_type: "openai",
                endpoint_id: "deepseek:global",
                default_model: Some("deepseek-chat"),
                fast_model: None,
                strong_model: None,
                base_url: Some("https://api.deepseek.com"),
                api_key: "sk-test",
                model_mapping: None,
                backup_provider_name: None,
                backup_provider_type: None,
                backup_endpoint_id: None,
                backup_default_model: None,
                backup_base_url: None,
                backup_api_key: None,
                backup_model_mapping: None,
            },
        )
        .await
        .expect("guide");

        assert_eq!(result.modes.len(), 1);

        let state = GatewayState::load(Path::new(config_path_str))
            .await
            .expect("load state");
        let services = state.list_services().await;
        let keys = state.list_api_keys().await;
        let default_mode = state.get_default_mode().await;

        assert_eq!(services.len(), 1);
        assert_eq!(keys.len(), 1);
        assert_eq!(default_mode.as_deref(), Some("default"));
        assert!(services.iter().any(|(id, _)| id == "default"));
    }

    #[tokio::test]
    async fn guide_configures_fallback_when_secondary_provider_given() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let config_path_str = config_path.to_str().expect("utf8 path");

        guide(
            config_path_str,
            GuideParams {
                service_id: None,
                service_name: None,
                provider_name: "deepseek-main",
                provider_type: "openai",
                endpoint_id: "deepseek:global",
                default_model: Some("deepseek-chat"),
                fast_model: None,
                strong_model: None,
                base_url: Some("https://api.deepseek.com"),
                api_key: "sk-primary",
                model_mapping: None,
                backup_provider_name: Some("openai-backup"),
                backup_provider_type: Some("openai"),
                backup_endpoint_id: Some("openai:global"),
                backup_default_model: Some("gpt-4o"),
                backup_base_url: Some("https://api.openai.com"),
                backup_api_key: Some("sk-backup"),
                backup_model_mapping: None,
            },
        )
        .await
        .expect("guide");

        let state = GatewayState::load(Path::new(config_path_str))
            .await
            .expect("load state");

        assert_eq!(state.get_routing_strategy("default").await, "fallback");

        let default_providers = state
            .select_all_providers_for_service("default", "openai")
            .await;

        assert_eq!(default_providers.len(), 2);
        assert_eq!(default_providers[0].name, "deepseek-main");
        assert_eq!(
            default_providers[0].endpoint_id.as_deref(),
            Some("deepseek:global")
        );
        assert_eq!(
            default_providers[0].default_model.as_deref(),
            Some("deepseek-chat")
        );
        assert_eq!(default_providers[1].name, "openai-backup");
        assert_eq!(
            default_providers[1].endpoint_id.as_deref(),
            Some("openai:global")
        );
        assert_eq!(
            default_providers[1].default_model.as_deref(),
            Some("gpt-4o")
        );
    }

    #[test]
    fn route_explanation_prefers_enabled_providers() {
        let explanation = render_route_explanation(&ModeView {
            id: "fast".to_string(),
            name: "Fast".to_string(),
            is_default: true,
            routing_strategy: "fallback".to_string(),
            providers: vec![
                ModeProvider {
                    name: "disabled-openai".to_string(),
                    provider_type: "openai".to_string(),
                    endpoint_id: Some("openai:global".to_string()),
                    base_url: Some("https://api.openai.com".to_string()),
                    default_model: Some("gpt-4o".to_string()),
                    model_mapping: None,
                    has_api_key: true,
                    is_enabled: false,
                    priority: 0,
                },
                ModeProvider {
                    name: "deepseek-main".to_string(),
                    provider_type: "openai".to_string(),
                    endpoint_id: Some("deepseek:global".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    default_model: Some("deepseek-chat".to_string()),
                    model_mapping: Some("fast-default=deepseek-chat".to_string()),
                    has_api_key: true,
                    is_enabled: true,
                    priority: 1,
                },
            ],
            keys: vec![ModeKey {
                key: "ugk_test".to_string(),
                is_active: true,
                quota_limit: None,
                qps_limit: None,
                concurrency_limit: None,
            }],
        });

        assert!(
            explanation
                .contains("Effective strategy: fallback across 1 provider(s) in priority order")
        );
        assert!(explanation.contains("deepseek-main"));
        assert!(explanation.contains("Disabled bound providers:"));
        assert!(!explanation.contains("1. disabled-openai"));
    }

    #[test]
    fn integration_output_can_filter_by_tool() {
        let output = render_integration_output_for_tool(
            &ModeView {
                id: "fast".to_string(),
                name: "Fast".to_string(),
                is_default: true,
                routing_strategy: "round_robin".to_string(),
                providers: vec![ModeProvider {
                    name: "deepseek-main".to_string(),
                    provider_type: "openai".to_string(),
                    endpoint_id: Some("deepseek:global".to_string()),
                    base_url: Some("https://api.deepseek.com".to_string()),
                    default_model: Some("deepseek-chat".to_string()),
                    model_mapping: None,
                    has_api_key: true,
                    is_enabled: true,
                    priority: 0,
                }],
                keys: vec![ModeKey {
                    key: "ugk_test".to_string(),
                    is_active: true,
                    quota_limit: None,
                    qps_limit: None,
                    concurrency_limit: None,
                }],
            },
            Some("ugk_test"),
            Some("127.0.0.1:3210"),
            IntegrationTool::Cursor,
        );

        assert!(output.contains("Cursor (OpenAI-compatible provider):"));
        assert!(!output.contains("Codex / codex-cli"));
        assert!(!output.contains("Python (openai SDK):"));
    }

    #[test]
    fn integration_output_supports_new_priority_tools() {
        let mode = ModeView {
            id: "fast".to_string(),
            name: "Fast".to_string(),
            is_default: true,
            routing_strategy: "round_robin".to_string(),
            providers: vec![ModeProvider {
                name: "deepseek-main".to_string(),
                provider_type: "openai".to_string(),
                endpoint_id: Some("deepseek:global".to_string()),
                base_url: Some("https://api.deepseek.com".to_string()),
                default_model: Some("deepseek-chat".to_string()),
                model_mapping: None,
                has_api_key: true,
                is_enabled: true,
                priority: 0,
            }],
            keys: vec![ModeKey {
                key: "ugk_test".to_string(),
                is_active: true,
                quota_limit: None,
                qps_limit: None,
                concurrency_limit: None,
            }],
        };

        let zed = render_integration_output_for_tool(
            &mode,
            Some("ugk_test"),
            Some("127.0.0.1:3210"),
            IntegrationTool::Zed,
        );
        assert!(zed.contains("Zed (settings.json or Agent Panel > Add Provider):"));
        assert!(zed.contains("\"openai_compatible\""));

        let openclaw = render_integration_output_for_tool(
            &mode,
            Some("ugk_test"),
            Some("127.0.0.1:3210"),
            IntegrationTool::OpenClaw,
        );
        assert!(openclaw.contains("OpenClaw (~/.openclaw/openclaw.json):"));
        assert!(openclaw.contains("api: \"openai-completions\""));
    }

    #[test]
    fn parse_integration_tool_supports_new_targets() {
        assert_eq!(
            parse_integration_tool(Some("openclaw")).expect("openclaw"),
            IntegrationTool::OpenClaw
        );
        assert_eq!(
            parse_integration_tool(Some("zed")).expect("zed"),
            IntegrationTool::Zed
        );
        assert_eq!(
            parse_integration_tool(Some("droid")).expect("droid"),
            IntegrationTool::Droid
        );
        assert_eq!(
            parse_integration_tool(Some("opencode")).expect("opencode"),
            IntegrationTool::OpenCode
        );
    }

    #[test]
    fn summarize_response_text_handles_sse_chunks() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"o\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"k\"}}]}\n\n",
            "data: [DONE]\n"
        );

        assert_eq!(summarize_response_text(body), "ok");
    }
}
