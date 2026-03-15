use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;
use std::{fmt::Write as _, path::Path};

use crate::{
    config::{GatewayState, ProviderModelOptions},
    routing::resolve_upstream,
    types::AppConfig,
};

pub struct QuickstartParams<'a> {
    pub service_id: Option<&'a str>,
    pub service_name: Option<&'a str>,
    pub provider_name: &'a str,
    pub provider_type: &'a str,
    pub endpoint_id: &'a str,
    pub default_model: Option<&'a str>,
    pub base_url: Option<&'a str>,
    pub api_key: &'a str,
    pub model_mapping: Option<&'a str>,
    pub backup_provider_name: Option<&'a str>,
    pub backup_provider_type: Option<&'a str>,
    pub backup_endpoint_id: Option<&'a str>,
    pub backup_default_model: Option<&'a str>,
    pub backup_base_url: Option<&'a str>,
    pub backup_api_key: Option<&'a str>,
    pub backup_model_mapping: Option<&'a str>,
}

pub struct QuickstartModeOutput {
    pub id: String,
    pub key: String,
}

pub struct QuickstartResult {
    pub modes: Vec<QuickstartModeOutput>,
}

struct QuickstartModePlan {
    id: String,
    name: String,
    routing_strategy: &'static str,
    bindings: Vec<(i64, i64)>,
}

#[derive(Clone)]
struct ModeProvider {
    name: String,
    provider_type: String,
    endpoint_id: Option<String>,
    base_url: Option<String>,
    default_model: Option<String>,
    model_mapping: Option<String>,
    has_api_key: bool,
    is_enabled: bool,
    priority: i64,
}

#[derive(Clone)]
struct ModeKey {
    key: String,
    is_active: bool,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
}

#[derive(Clone)]
struct ModeView {
    id: String,
    name: String,
    is_default: bool,
    routing_strategy: String,
    providers: Vec<ModeProvider>,
    keys: Vec<ModeKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntegrationTool {
    All,
    Cursor,
    Codex,
    ClaudeCode,
    Env,
    Python,
    Node,
    Curl,
    Anthropic,
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}…{}", &key[..4], &key[key.len() - 4..])
}

fn format_i64_limit(limit: Option<i64>) -> String {
    limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

fn format_f64_limit(limit: Option<f64>) -> String {
    limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

fn user_bind_address(bind: &str) -> String {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return bind.to_string();
    };

    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        _ => host,
    };
    format!("{host}:{port}")
}

fn user_openai_base_url(bind_override: Option<&str>) -> String {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    format!("http://{}/v1", user_bind_address(&bind))
}

fn user_anthropic_base_url(bind_override: Option<&str>) -> String {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    format!("http://{}", user_bind_address(&bind))
}

async fn load_mode_views(config_path: &str) -> Result<Vec<ModeView>> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let default_mode = state.get_default_mode().await.unwrap_or_default();
    let guard = state.inner.read().await;

    let mut modes = Vec::new();
    for service in &guard.file.services {
        let mut providers: Vec<ModeProvider> = guard
            .file
            .bindings
            .iter()
            .filter(|binding| binding.service_id == service.id)
            .map(|binding| {
                let provider = guard
                    .file
                    .providers
                    .iter()
                    .find(|provider| provider.name == binding.provider_name);
                ModeProvider {
                    name: binding.provider_name.clone(),
                    provider_type: provider
                        .map(|provider| provider.provider_type.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    endpoint_id: provider.and_then(|provider| {
                        if provider.endpoint_id.is_empty() {
                            None
                        } else {
                            Some(provider.endpoint_id.clone())
                        }
                    }),
                    base_url: provider.and_then(|provider| {
                        if provider.base_url.is_empty() {
                            None
                        } else {
                            Some(provider.base_url.clone())
                        }
                    }),
                    default_model: provider.and_then(|provider| {
                        if provider.default_model.is_empty() {
                            None
                        } else {
                            Some(provider.default_model.clone())
                        }
                    }),
                    model_mapping: provider.and_then(|provider| {
                        if provider.model_mapping.is_empty() {
                            None
                        } else {
                            Some(provider.model_mapping.clone())
                        }
                    }),
                    has_api_key: provider
                        .map(|provider| !provider.api_key.is_empty())
                        .unwrap_or(false),
                    is_enabled: provider
                        .map(|provider| provider.is_enabled)
                        .unwrap_or(false),
                    priority: binding.priority,
                }
            })
            .collect();
        providers.sort_by_key(|provider| provider.priority);

        let keys = guard
            .file
            .api_keys
            .iter()
            .filter(|key| key.service_id == service.id)
            .map(|key| ModeKey {
                key: key.key.clone(),
                is_active: key.is_active,
                quota_limit: key.quota_limit,
                qps_limit: key.qps_limit,
                concurrency_limit: key.concurrency_limit,
            })
            .collect();

        modes.push(ModeView {
            id: service.id.clone(),
            name: service.name.clone(),
            is_default: !default_mode.is_empty() && default_mode == service.id,
            routing_strategy: service.routing_strategy.clone(),
            providers,
            keys,
        });
    }

    Ok(modes)
}

fn supported_protocols(mode: &ModeView) -> Vec<&'static str> {
    let mut protocols = Vec::new();
    if mode
        .providers
        .iter()
        .any(|provider| provider.is_enabled && provider.provider_type == "openai")
    {
        protocols.push("openai");
    }
    if mode
        .providers
        .iter()
        .any(|provider| provider.is_enabled && provider.provider_type == "anthropic")
    {
        protocols.push("anthropic");
    }
    protocols
}

fn mode_providers_for<'a>(mode: &'a ModeView, protocol: &str) -> Vec<&'a ModeProvider> {
    mode.providers
        .iter()
        .filter(|provider| provider.is_enabled && provider.provider_type == protocol)
        .collect()
}

fn effective_default_mode_id(modes: &[ModeView]) -> Option<&str> {
    modes
        .iter()
        .find(|mode| mode.is_default)
        .map(|mode| mode.id.as_str())
        .or_else(|| {
            modes
                .iter()
                .find(|mode| mode.id == "default")
                .map(|mode| mode.id.as_str())
        })
        .or_else(|| {
            if modes.len() == 1 {
                Some(modes[0].id.as_str())
            } else {
                None
            }
        })
}

fn select_mode<'a>(modes: &'a [ModeView], requested_mode: Option<&str>) -> Result<&'a ModeView> {
    if modes.is_empty() {
        bail!("no modes configured; run `ug quickstart` first")
    }

    if let Some(mode) = requested_mode {
        return modes
            .iter()
            .find(|candidate| candidate.id == mode)
            .with_context(|| format!("mode '{}' not found", mode));
    }

    if let Some(default_mode_id) = effective_default_mode_id(modes) {
        return modes
            .iter()
            .find(|candidate| candidate.id == default_mode_id)
            .with_context(|| format!("default mode '{}' not found", default_mode_id));
    }

    let ids = modes
        .iter()
        .map(|mode| mode.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("multiple modes configured ({ids}); use --mode")
}

fn parse_integration_tool(tool: Option<&str>) -> Result<IntegrationTool> {
    match tool.map(|tool| tool.trim().to_ascii_lowercase()) {
        None => Ok(IntegrationTool::All),
        Some(tool) if tool.is_empty() || tool == "all" => Ok(IntegrationTool::All),
        Some(tool) if tool == "cursor" => Ok(IntegrationTool::Cursor),
        Some(tool) if tool == "codex" => Ok(IntegrationTool::Codex),
        Some(tool) if tool == "claude-code" || tool == "claudecode" => {
            Ok(IntegrationTool::ClaudeCode)
        }
        Some(tool) if tool == "env" || tool == "shell" => Ok(IntegrationTool::Env),
        Some(tool) if tool == "python" => Ok(IntegrationTool::Python),
        Some(tool) if tool == "node" || tool == "javascript" => Ok(IntegrationTool::Node),
        Some(tool) if tool == "curl" => Ok(IntegrationTool::Curl),
        Some(tool) if tool == "anthropic" => Ok(IntegrationTool::Anthropic),
        Some(tool) => bail!(
            "unknown integration target '{}'; use one of: cursor, codex, claude-code, env, python, node, curl, anthropic",
            tool
        ),
    }
}

fn provider_default_model<'a>(provider: &'a ModeProvider, fallback: &'a str) -> &'a str {
    provider
        .default_model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or(fallback)
}

fn render_openai_tool_settings(
    out: &mut String,
    title: &str,
    base_url: &str,
    key: Option<&str>,
    model: &str,
) {
    let _ = writeln!(out, "{}:", title);
    let _ = writeln!(out, "  Base URL: {}", base_url);
    let _ = writeln!(out, "  API Key: {}", key.unwrap_or("<gateway api key>"));
    let _ = writeln!(out, "  Model: {}", model);
}

fn render_openai_env_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Shell environment:");
    let _ = writeln!(out, "  export OPENAI_BASE_URL={}", base_url);
    let _ = writeln!(
        out,
        "  export OPENAI_API_KEY={}",
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(out, "  export OPENAI_MODEL={}", model);
}

fn render_openai_python_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Python (openai SDK):");
    let _ = writeln!(out, "  from openai import OpenAI");
    let _ = writeln!(
        out,
        "  client = OpenAI(base_url=\"{}\", api_key=\"{}\")",
        base_url,
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(
        out,
        "  print(client.chat.completions.create(model=\"{}\", messages=[{{\"role\": \"user\", \"content\": \"hello\"}}]).choices[0].message.content)",
        model
    );
}

fn render_openai_node_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Node (openai SDK):");
    let _ = writeln!(out, "  import OpenAI from \"openai\";");
    let _ = writeln!(
        out,
        "  const client = new OpenAI({{ baseURL: \"{}\", apiKey: \"{}\" }});",
        base_url,
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(
        out,
        "  const response = await client.chat.completions.create({{ model: \"{}\", messages: [{{ role: \"user\", content: \"hello\" }}] }});",
        model
    );
    let _ = writeln!(out, "  console.log(response.choices[0].message.content);");
}

fn render_openai_curl_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "curl:");
    let _ = writeln!(
        out,
        "  curl -s {}/chat/completions -H \"Authorization: Bearer {}\" -H \"Content-Type: application/json\" -d '{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hello\"}}]}}'",
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
}

fn render_anthropic_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Anthropic-compatible clients:");
    let _ = writeln!(out, "  Base URL: {}", base_url);
    let _ = writeln!(out, "  x-api-key: {}", key.unwrap_or("<gateway api key>"));
    let _ = writeln!(out, "  Model: {}", model);
    let _ = writeln!(out, "  curl:");
    let _ = writeln!(
        out,
        "    curl -s {}/v1/messages -H \"x-api-key: {}\" -H \"anthropic-version: 2023-06-01\" -H \"Content-Type: application/json\" -d '{{\"model\":\"{}\",\"max_tokens\":64,\"messages\":[{{\"role\":\"user\",\"content\":\"hello\"}}]}}'",
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
}

fn render_integration_output_for_tool(
    mode: &ModeView,
    key: Option<&str>,
    bind_override: Option<&str>,
    tool: IntegrationTool,
) -> String {
    let openai_provider = mode
        .providers
        .iter()
        .find(|provider| provider.is_enabled && provider.provider_type == "openai");
    let anthropic_provider = mode
        .providers
        .iter()
        .find(|provider| provider.is_enabled && provider.provider_type == "anthropic");

    let mut out = String::new();
    let protocols = supported_protocols(mode);

    let _ = writeln!(&mut out, "Mode: {} ({})", mode.id, mode.name);
    let _ = writeln!(&mut out, "Routing: {}", mode.routing_strategy);
    let _ = writeln!(
        &mut out,
        "Protocols: {}",
        if protocols.is_empty() {
            "none".to_string()
        } else {
            protocols.join(", ")
        }
    );

    if let Some(key) = key {
        let _ = writeln!(&mut out, "Gateway API Key: {}", key);
    } else {
        let _ = writeln!(
            &mut out,
            "Gateway API Key: <create one with ug create-api-key>"
        );
    }

    if let Some(provider) = openai_provider {
        let model = provider_default_model(provider, "your-model");
        let base_url = user_openai_base_url(bind_override);
        let _ = writeln!(&mut out);
        let wants_openai = matches!(
            tool,
            IntegrationTool::All
                | IntegrationTool::Cursor
                | IntegrationTool::Codex
                | IntegrationTool::ClaudeCode
                | IntegrationTool::Env
                | IntegrationTool::Python
                | IntegrationTool::Node
                | IntegrationTool::Curl
        );

        if wants_openai {
            let _ = writeln!(&mut out, "OpenAI-compatible integrations:");
            match tool {
                IntegrationTool::All => {
                    render_openai_tool_settings(
                        &mut out,
                        "  Cursor (OpenAI-compatible provider)",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  Codex / codex-cli",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  Claude Code custom OpenAI endpoint",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_env_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_python_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_node_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_curl_block(&mut out, &base_url, key, model);
                }
                IntegrationTool::Cursor => render_openai_tool_settings(
                    &mut out,
                    "  Cursor (OpenAI-compatible provider)",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::Codex => render_openai_tool_settings(
                    &mut out,
                    "  Codex / codex-cli",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::ClaudeCode => render_openai_tool_settings(
                    &mut out,
                    "  Claude Code custom OpenAI endpoint",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::Env => render_openai_env_block(&mut out, &base_url, key, model),
                IntegrationTool::Python => {
                    render_openai_python_block(&mut out, &base_url, key, model)
                }
                IntegrationTool::Node => render_openai_node_block(&mut out, &base_url, key, model),
                IntegrationTool::Curl => render_openai_curl_block(&mut out, &base_url, key, model),
                IntegrationTool::Anthropic => {}
            }
        }
    } else if matches!(
        tool,
        IntegrationTool::Cursor
            | IntegrationTool::Codex
            | IntegrationTool::ClaudeCode
            | IntegrationTool::Env
            | IntegrationTool::Python
            | IntegrationTool::Node
            | IntegrationTool::Curl
    ) {
        let _ = writeln!(&mut out);
        let _ = writeln!(
            &mut out,
            "No enabled OpenAI-compatible provider is bound to this mode."
        );
    }

    if let Some(provider) = anthropic_provider {
        let model = provider_default_model(provider, "your-model");
        let base_url = user_anthropic_base_url(bind_override);
        if matches!(tool, IntegrationTool::All | IntegrationTool::Anthropic) {
            let _ = writeln!(&mut out);
            render_anthropic_block(&mut out, &base_url, key, model);
        }
    } else if matches!(tool, IntegrationTool::Anthropic) {
        let _ = writeln!(&mut out);
        let _ = writeln!(
            &mut out,
            "No enabled Anthropic-compatible provider is bound to this mode."
        );
    }

    out.trim_end().to_string()
}

#[cfg(test)]
fn planned_modes(service_id: Option<&str>, service_name: Option<&str>) -> Vec<(String, String)> {
    if let Some(service_id) = service_id {
        return vec![(
            service_id.to_string(),
            service_name.unwrap_or(service_id).to_string(),
        )];
    }

    vec![
        ("fast".to_string(), "Fast".to_string()),
        ("strong".to_string(), "Strong".to_string()),
        ("backup".to_string(), "Backup".to_string()),
    ]
}

fn quickstart_mode_plans(
    service_id: Option<&str>,
    service_name: Option<&str>,
    primary_provider_id: i64,
    secondary_provider_id: Option<i64>,
) -> Vec<QuickstartModePlan> {
    if let Some(service_id) = service_id {
        let mut bindings = vec![(primary_provider_id, 0)];
        let routing_strategy = if let Some(secondary_provider_id) = secondary_provider_id {
            bindings.push((secondary_provider_id, 1));
            "fallback"
        } else {
            "round_robin"
        };

        return vec![QuickstartModePlan {
            id: service_id.to_string(),
            name: service_name.unwrap_or(service_id).to_string(),
            routing_strategy,
            bindings,
        }];
    }

    let strong_bindings = secondary_provider_id
        .map(|provider_id| vec![(provider_id, 0)])
        .unwrap_or_else(|| vec![(primary_provider_id, 0)]);

    let mut backup_bindings = vec![(primary_provider_id, 0)];
    if let Some(secondary_provider_id) = secondary_provider_id {
        backup_bindings.push((secondary_provider_id, 1));
    }

    vec![
        QuickstartModePlan {
            id: "fast".to_string(),
            name: "Fast".to_string(),
            routing_strategy: "round_robin",
            bindings: vec![(primary_provider_id, 0)],
        },
        QuickstartModePlan {
            id: "strong".to_string(),
            name: "Strong".to_string(),
            routing_strategy: "round_robin",
            bindings: strong_bindings,
        },
        QuickstartModePlan {
            id: "backup".to_string(),
            name: "Backup".to_string(),
            routing_strategy: "fallback",
            bindings: backup_bindings,
        },
    ]
}

fn pick_mode_key(mode: &ModeView) -> Result<String> {
    mode.keys
        .iter()
        .find(|key| key.is_active)
        .or_else(|| mode.keys.first())
        .map(|key| key.key.clone())
        .with_context(|| {
            format!(
                "mode '{}' has no API key; create one with `ug create-api-key`",
                mode.id
            )
        })
}

fn pick_mode_protocol<'a>(mode: &'a ModeView, requested: Option<&str>) -> Result<&'a str> {
    let protocols = supported_protocols(mode);
    if protocols.is_empty() {
        bail!("mode '{}' has no supported providers", mode.id);
    }

    if let Some(requested) = requested {
        let requested = requested.trim().to_ascii_lowercase();
        return protocols
            .into_iter()
            .find(|protocol| *protocol == requested)
            .with_context(|| {
                format!(
                    "mode '{}' does not support protocol '{}'; available: {}",
                    mode.id,
                    requested,
                    supported_protocols(mode).join(", ")
                )
            });
    }

    Ok(protocols[0])
}

fn route_strategy_summary(mode: &ModeView, providers: &[&ModeProvider]) -> String {
    if providers.is_empty() {
        return "no enabled providers".to_string();
    }

    if mode.routing_strategy == "fallback" {
        return format!(
            "fallback across {} provider(s) in priority order",
            providers.len()
        );
    }

    if providers.len() == 1 {
        "single provider".to_string()
    } else {
        format!("round_robin across {} provider(s)", providers.len())
    }
}

fn render_route_explanation(mode: &ModeView) -> String {
    let mut out = String::new();
    let protocols = supported_protocols(mode);

    let _ = writeln!(&mut out, "Mode: {} ({})", mode.id, mode.name);
    let _ = writeln!(&mut out, "Routing: {}", mode.routing_strategy);
    let _ = writeln!(
        &mut out,
        "Protocols: {}",
        if protocols.is_empty() {
            "none".to_string()
        } else {
            protocols.join(", ")
        }
    );

    if protocols.is_empty() {
        let _ = writeln!(&mut out, "No enabled providers are bound to this mode.");
        return out.trim_end().to_string();
    }

    for protocol in protocols {
        let providers = mode_providers_for(mode, protocol);
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "{}:", protocol);
        let _ = writeln!(
            &mut out,
            "  Effective strategy: {}",
            route_strategy_summary(mode, &providers)
        );

        for (index, provider) in providers.iter().enumerate() {
            let (resolved_base_url, family_id) =
                resolve_upstream(provider.base_url.clone(), provider.endpoint_id.as_deref())
                    .unwrap_or_else(|| {
                        (
                            provider
                                .base_url
                                .clone()
                                .unwrap_or_else(|| "(unresolved)".to_string()),
                            None,
                        )
                    });

            let _ = writeln!(&mut out, "  {}. {}", index + 1, provider.name);
            let _ = writeln!(&mut out, "     provider_type: {}", provider.provider_type);
            let _ = writeln!(
                &mut out,
                "     endpoint_id: {}",
                provider.endpoint_id.as_deref().unwrap_or("-")
            );
            let _ = writeln!(
                &mut out,
                "     default_model: {}",
                provider.default_model.as_deref().unwrap_or("-")
            );
            let _ = writeln!(&mut out, "     resolved_base_url: {}", resolved_base_url);
            let _ = writeln!(
                &mut out,
                "     family: {}",
                family_id.as_deref().unwrap_or("-")
            );
            let _ = writeln!(
                &mut out,
                "     model_mapping: {}",
                provider.model_mapping.as_deref().unwrap_or("-")
            );
            let _ = writeln!(&mut out, "     binding_priority: {}", provider.priority);
        }
    }

    let disabled: Vec<&ModeProvider> = mode
        .providers
        .iter()
        .filter(|provider| !provider.is_enabled)
        .collect();
    if !disabled.is_empty() {
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "Disabled bound providers:");
        for provider in disabled {
            let _ = writeln!(
                &mut out,
                "  - {} ({})",
                provider.name, provider.provider_type
            );
        }
    }

    out.trim_end().to_string()
}

fn summarize_response_text(body: &str) -> String {
    let streamed = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|line| !line.trim().is_empty() && *line != "[DONE]")
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| {
            value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .or_else(|| {
                    value
                        .pointer("/choices/0/message/content")
                        .and_then(Value::as_str)
                })
                .or_else(|| value.pointer("/content/0/text").and_then(Value::as_str))
                .map(ToOwned::to_owned)
        })
        .collect::<String>();
    if !streamed.trim().is_empty() {
        return if streamed.len() > 160 {
            format!("{}...", &streamed[..160])
        } else {
            streamed
        };
    }

    let parsed = serde_json::from_str::<Value>(body).ok();
    let text = parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .or_else(|| value.pointer("/content/0/text").and_then(Value::as_str))
                .or_else(|| value.pointer("/error/message").and_then(Value::as_str))
        })
        .unwrap_or(body)
        .trim();

    if text.len() > 160 {
        format!("{}...", &text[..160])
    } else {
        text.to_string()
    }
}

async fn gateway_health_status(bind_override: Option<&str>) -> String {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    let url = format!("http://{}/health", user_bind_address(&bind));
    let client = Client::new();

    match client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                return format!("gateway responded with status {} at {}", status, url);
            }

            match response.text().await {
                Ok(body) => {
                    let message = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("status")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                        })
                        .unwrap_or_else(|| "ok".to_string());
                    format!("reachable ({}) at {}", message, url)
                }
                Err(err) => format!(
                    "gateway reachable at {}, but health body could not be read: {}",
                    url, err
                ),
            }
        }
        Err(err) => format!("not reachable at {} ({})", url, err),
    }
}

fn provider_readiness(provider: &ModeProvider) -> String {
    let upstream =
        if resolve_upstream(provider.base_url.clone(), provider.endpoint_id.as_deref()).is_some() {
            "resolved upstream"
        } else {
            "missing upstream"
        };
    let api_key = if provider.has_api_key {
        "upstream key configured"
    } else {
        "missing upstream key"
    };
    format!("{}, {}", upstream, api_key)
}

pub async fn create_service(config_path: &str, service_id: &str, name: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.create_service(service_id, name).await;
    state.persist_if_dirty().await
}

pub async fn create_provider(
    config_path: &str,
    name: &str,
    provider_type: &str,
    endpoint_id: &str,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) -> Result<i64> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let id = state
        .create_provider(
            name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
        )
        .await;
    state.persist_if_dirty().await?;
    Ok(id)
}

pub async fn bind_provider(config_path: &str, service_id: &str, provider_id: i64) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state
        .bind_provider_to_service(service_id, provider_id)
        .await
        .with_context(|| format!("bind provider_id {} to service {}", provider_id, service_id))?;
    state.persist_if_dirty().await
}

pub async fn create_api_key(
    config_path: &str,
    key: &str,
    service_id: &str,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state
        .create_api_key(key, service_id, quota_limit, qps_limit, concurrency_limit)
        .await;
    state.persist_if_dirty().await
}

pub async fn quickstart(
    config_path: &str,
    params: QuickstartParams<'_>,
) -> Result<QuickstartResult> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let primary_provider_id = state
        .create_provider_with_models(
            params.provider_name,
            params.provider_type,
            params.endpoint_id,
            params.base_url,
            params.api_key,
            ProviderModelOptions {
                default_model: params.default_model,
                model_mapping: params.model_mapping,
            },
        )
        .await;

    let secondary_provider_id = match (
        params.backup_provider_name,
        params.backup_provider_type,
        params.backup_endpoint_id,
        params.backup_api_key,
    ) {
        (Some(name), Some(provider_type), Some(endpoint_id), Some(api_key)) => Some(
            state
                .create_provider_with_models(
                    name,
                    provider_type,
                    endpoint_id,
                    params.backup_base_url,
                    api_key,
                    ProviderModelOptions {
                        default_model: params.backup_default_model,
                        model_mapping: params.backup_model_mapping,
                    },
                )
                .await,
        ),
        (None, None, None, None) => None,
        _ => bail!("backup provider requires name, provider_type, endpoint_id, and api_key"),
    };

    let planned = quickstart_mode_plans(
        params.service_id,
        params.service_name,
        primary_provider_id,
        secondary_provider_id,
    );
    let default_mode = planned.first().map(|mode| mode.id.clone());
    let mut modes = Vec::new();
    for plan in planned {
        let key = format!("ugk_{}", hex::encode(rand::random::<[u8; 16]>()));
        state.create_service(&plan.id, &plan.name).await;
        state
            .set_service_routing_strategy(&plan.id, plan.routing_strategy)
            .await?;
        for (provider_id, priority) in &plan.bindings {
            state
                .bind_provider_to_service_with_priority(&plan.id, *provider_id, *priority)
                .await?;
        }
        state.create_api_key(&key, &plan.id, None, None, None).await;
        modes.push(QuickstartModeOutput { id: plan.id, key });
    }

    if let Some(default_mode) = default_mode {
        state.set_default_mode(&default_mode).await?;
    }

    state.persist_if_dirty().await?;
    Ok(QuickstartResult { modes })
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

pub async fn list_modes(config_path: &str) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    if modes.is_empty() {
        println!("No modes configured. Run `ug quickstart` first.");
        return Ok(());
    }

    let default_mode = effective_default_mode_id(&modes).map(ToOwned::to_owned);

    println!("Modes:");
    for mode in modes {
        let protocols = supported_protocols(&mode);
        println!(
            "  - {}{} ({}) routing={} providers={} keys={} protocols={}",
            mode.id,
            if default_mode.as_deref() == Some(mode.id.as_str()) {
                " [default]"
            } else {
                ""
            },
            mode.name,
            mode.routing_strategy,
            mode.providers.len(),
            mode.keys.iter().filter(|key| key.is_active).count(),
            if protocols.is_empty() {
                "none".to_string()
            } else {
                protocols.join(", ")
            }
        );
    }
    Ok(())
}

pub async fn show_mode(config_path: &str, mode_id: &str) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let default_mode = effective_default_mode_id(&modes).map(ToOwned::to_owned);
    let mode = select_mode(&modes, Some(mode_id))?;

    println!("Mode: {}", mode.id);
    println!("Name: {}", mode.name);
    println!(
        "Default: {}",
        default_mode.as_deref() == Some(mode.id.as_str())
    );
    println!("Routing: {}", mode.routing_strategy);

    let protocols = supported_protocols(mode);
    println!(
        "Protocols: {}",
        if protocols.is_empty() {
            "none".to_string()
        } else {
            protocols.join(", ")
        }
    );

    if mode.providers.is_empty() {
        println!("Providers: none");
    } else {
        println!("Providers:");
        for provider in &mode.providers {
            println!(
                "  - name={} type={} enabled={} priority={} endpoint={} default_model={} base_url={}",
                provider.name,
                provider.provider_type,
                provider.is_enabled,
                provider.priority,
                provider.endpoint_id.as_deref().unwrap_or("-"),
                provider.default_model.as_deref().unwrap_or("-"),
                provider.base_url.as_deref().unwrap_or("(default)"),
            );
        }
    }

    if mode.keys.is_empty() {
        println!("API Keys: none");
    } else {
        println!("API Keys:");
        for key in &mode.keys {
            println!(
                "  - key={} active={} quota={} qps={} concurrency={}",
                mask_key(&key.key),
                key.is_active,
                format_i64_limit(key.quota_limit),
                format_f64_limit(key.qps_limit),
                format_i64_limit(key.concurrency_limit),
            );
        }
    }

    Ok(())
}

pub async fn explain_route(config_path: &str, mode_id: Option<&str>) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id)?;
    println!("{}", render_route_explanation(mode));
    Ok(())
}

pub async fn use_mode(config_path: &str, mode_id: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.set_default_mode(mode_id).await?;
    state.persist_if_dirty().await?;
    println!("Default mode set to '{}'", mode_id);
    Ok(())
}

pub async fn doctor(
    config_path: &str,
    mode_id: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    let config_exists = Path::new(config_path).exists();
    let modes = load_mode_views(config_path).await?;
    let default_mode = effective_default_mode_id(&modes).map(ToOwned::to_owned);
    let health = gateway_health_status(bind_override).await;
    let bind_display = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);

    println!("UniGateway doctor");
    println!("Config path: {}", config_path);
    println!(
        "Config file: {}",
        if config_exists {
            "present"
        } else {
            "missing (using in-memory defaults if started)"
        }
    );
    println!("Gateway bind: {}", bind_display);
    println!("Gateway health: {}", health);

    if modes.is_empty() {
        println!("Modes: none");
        println!("Next step: run `ug quickstart`");
        return Ok(());
    }

    let selected: Vec<&ModeView> = if let Some(mode_id) = mode_id {
        vec![select_mode(&modes, Some(mode_id))?]
    } else {
        modes.iter().collect()
    };

    println!("Modes checked: {}", selected.len());
    for mode in selected {
        let protocols = supported_protocols(mode);
        let active_keys = mode.keys.iter().filter(|key| key.is_active).count();
        println!();
        println!("- {} ({})", mode.id, mode.name);
        println!(
            "  default: {}",
            default_mode.as_deref() == Some(mode.id.as_str())
        );
        println!("  routing: {}", mode.routing_strategy);
        println!("  active_keys: {} / {}", active_keys, mode.keys.len());
        println!(
            "  protocols: {}",
            if protocols.is_empty() {
                "none".to_string()
            } else {
                protocols.join(", ")
            }
        );

        if active_keys == 0 {
            println!("  warning: no active gateway key for this mode");
        }

        for protocol in protocols {
            let providers = mode_providers_for(mode, protocol);
            println!(
                "  {} route: {}",
                protocol,
                route_strategy_summary(mode, &providers)
            );

            for provider in providers {
                let (resolved_base_url, family_id) =
                    resolve_upstream(provider.base_url.clone(), provider.endpoint_id.as_deref())
                        .unwrap_or_else(|| {
                            (
                                provider
                                    .base_url
                                    .clone()
                                    .unwrap_or_else(|| "(unresolved)".to_string()),
                                None,
                            )
                        });
                println!(
                    "    - {} -> {} | family={} | {}",
                    provider.name,
                    resolved_base_url,
                    family_id.as_deref().unwrap_or("-"),
                    provider_readiness(provider)
                );
            }
        }

        let disabled = mode
            .providers
            .iter()
            .filter(|provider| !provider.is_enabled)
            .count();
        if disabled > 0 {
            println!("  note: {} bound provider(s) are disabled", disabled);
        }

        println!("  next:");
        println!("    ug route explain {}", mode.id);
        println!("    ug integrations --mode {}", mode.id);
        println!("    ug test --mode {}", mode.id);
    }

    Ok(())
}

pub async fn print_integrations(
    config_path: &str,
    mode_id: Option<&str>,
    tool: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    print_integrations_with_key(config_path, mode_id, tool, None, bind_override).await
}

pub async fn print_integrations_with_key(
    config_path: &str,
    mode_id: Option<&str>,
    tool: Option<&str>,
    preferred_key: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id)?;
    let tool = parse_integration_tool(tool)?;
    let key = preferred_key.map(ToOwned::to_owned).or_else(|| {
        mode.keys
            .iter()
            .find(|key| key.is_active)
            .or_else(|| mode.keys.first())
            .map(|key| key.key.clone())
    });

    println!(
        "{}",
        render_integration_output_for_tool(mode, key.as_deref(), bind_override, tool)
    );
    Ok(())
}

pub async fn test_mode(
    config_path: &str,
    mode_id: Option<&str>,
    protocol: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id)?;
    let key = pick_mode_key(mode)?;
    let protocol = pick_mode_protocol(mode, protocol)?;
    let client = Client::new();

    let (url, request) = match protocol {
        "openai" => {
            let provider = mode_providers_for(mode, "openai")
                .into_iter()
                .next()
                .with_context(|| format!("mode '{}' has no openai provider", mode.id))?;
            let model = provider_default_model(provider, "gpt-4o-mini");
            (
                format!("{}/chat/completions", user_openai_base_url(bind_override)),
                client
                    .post(format!(
                        "{}/chat/completions",
                        user_openai_base_url(bind_override)
                    ))
                    .bearer_auth(&key)
                    .json(&serde_json::json!({
                        "model": model,
                        "messages": [{"role": "user", "content": "reply with ok"}],
                        "max_tokens": 16,
                        "stream": true
                    })),
            )
        }
        "anthropic" => {
            let provider = mode_providers_for(mode, "anthropic")
                .into_iter()
                .next()
                .with_context(|| format!("mode '{}' has no anthropic provider", mode.id))?;
            let model = provider_default_model(provider, "claude-3-5-sonnet-latest");
            (
                format!("{}/v1/messages", user_anthropic_base_url(bind_override)),
                client
                    .post(format!(
                        "{}/v1/messages",
                        user_anthropic_base_url(bind_override)
                    ))
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&serde_json::json!({
                        "model": model,
                        "max_tokens": 32,
                        "messages": [{"role": "user", "content": "reply with ok"}],
                        "stream": true
                    })),
            )
        }
        _ => bail!("unsupported protocol '{}'", protocol),
    };

    let response = request.send().await.with_context(|| {
        format!(
            "failed to connect to {}. Start the gateway with `ug serve` and try again",
            url
        )
    })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("read gateway response body")?;

    if !status.is_success() {
        bail!(
            "smoke test failed for mode '{}' via {} (status {}): {}",
            mode.id,
            protocol,
            status,
            summarize_response_text(&body)
        );
    }

    println!(
        "Mode '{}' passed {} smoke test: {}",
        mode.id,
        protocol,
        summarize_response_text(&body)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        IntegrationTool, ModeKey, ModeProvider, ModeView, QuickstartParams,
        effective_default_mode_id, planned_modes, quickstart, render_integration_output_for_tool,
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
    fn planned_modes_defaults_to_personal_bundle() {
        let modes = planned_modes(None, None);
        assert_eq!(modes.len(), 3);
        assert_eq!(modes[0].0, "fast");
        assert_eq!(modes[1].0, "strong");
        assert_eq!(modes[2].0, "backup");
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
    async fn quickstart_creates_personal_bundle_when_mode_not_specified() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let config_path_str = config_path.to_str().expect("utf8 path");

        let result = quickstart(
            config_path_str,
            QuickstartParams {
                service_id: None,
                service_name: None,
                provider_name: "deepseek-main",
                provider_type: "openai",
                endpoint_id: "deepseek:global",
                default_model: Some("deepseek-chat"),
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
        .expect("quickstart");

        assert_eq!(result.modes.len(), 3);

        let state = GatewayState::load(Path::new(config_path_str))
            .await
            .expect("load state");
        let services = state.list_services().await;
        let keys = state.list_api_keys().await;
        let default_mode = state.get_default_mode().await;

        assert_eq!(services.len(), 3);
        assert_eq!(keys.len(), 3);
        assert_eq!(default_mode.as_deref(), Some("fast"));
        assert!(services.iter().any(|(id, _)| id == "fast"));
        assert!(services.iter().any(|(id, _)| id == "strong"));
        assert!(services.iter().any(|(id, _)| id == "backup"));
    }

    #[tokio::test]
    async fn quickstart_uses_secondary_provider_for_strong_and_backup_modes() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("config.toml");
        let config_path_str = config_path.to_str().expect("utf8 path");

        quickstart(
            config_path_str,
            QuickstartParams {
                service_id: None,
                service_name: None,
                provider_name: "deepseek-main",
                provider_type: "openai",
                endpoint_id: "deepseek:global",
                default_model: Some("deepseek-chat"),
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
        .expect("quickstart");

        let state = GatewayState::load(Path::new(config_path_str))
            .await
            .expect("load state");

        assert_eq!(state.get_routing_strategy("fast").await, "round_robin");
        assert_eq!(state.get_routing_strategy("strong").await, "round_robin");
        assert_eq!(state.get_routing_strategy("backup").await, "fallback");

        let fast = state
            .select_all_providers_for_service("fast", "openai")
            .await;
        let strong = state
            .select_all_providers_for_service("strong", "openai")
            .await;
        let backup = state
            .select_all_providers_for_service("backup", "openai")
            .await;

        assert_eq!(fast.len(), 1);
        assert_eq!(fast[0].name, "deepseek-main");
        assert_eq!(fast[0].endpoint_id.as_deref(), Some("deepseek:global"));
        assert_eq!(fast[0].default_model.as_deref(), Some("deepseek-chat"));
        assert_eq!(strong.len(), 1);
        assert_eq!(strong[0].name, "openai-backup");
        assert_eq!(strong[0].endpoint_id.as_deref(), Some("openai:global"));
        assert_eq!(strong[0].default_model.as_deref(), Some("gpt-4o"));
        assert_eq!(backup.len(), 2);
        assert_eq!(backup[0].name, "deepseek-main");
        assert_eq!(backup[1].name, "openai-backup");
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
    fn summarize_response_text_handles_sse_chunks() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"o\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"k\"}}]}\n\n",
            "data: [DONE]\n"
        );

        assert_eq!(summarize_response_text(body), "ok");
    }
}
