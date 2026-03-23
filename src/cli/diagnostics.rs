use anyhow::{Context, Result, bail};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

use crate::{
    config::{ModeProvider, ModeView},
    routing::resolve_upstream,
    types::AppConfig,
};

use super::{
    modes::{
        effective_default_mode_id, load_mode_views, mode_providers_for, pick_mode_key,
        pick_mode_protocol, provider_default_model, select_mode, supported_protocols,
        user_anthropic_base_url, user_bind_address, user_openai_base_url,
    },
    render::routes::route_strategy_summary,
};

pub(crate) fn summarize_response_text(body: &str) -> String {
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

async fn gateway_health_status(bind_override: Option<&str>) -> (bool, String) {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    let url = format!("http://{}/health", user_bind_address(&bind));
    let client = Client::new();

    match client.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                return (
                    false,
                    format!("gateway responded with status {} at {}", status, url),
                );
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
                    (true, format!("reachable ({}) at {}", message, url))
                }
                Err(err) => (
                    false,
                    format!(
                        "gateway reachable at {}, but health body could not be read: {}",
                        url, err
                    ),
                ),
            }
        }
        Err(_) => (false, format!("not reachable at {}", url)),
    }
}

fn provider_readiness(provider: &ModeProvider) -> String {
    let upstream =
        if resolve_upstream(provider.base_url.clone(), provider.endpoint_id.as_deref()).is_some() {
            style("✓ resolved").green()
        } else {
            style("✗ missing upstream").red()
        };
    let api_key = if provider.has_api_key {
        style("✓ key configured").green()
    } else {
        style("✗ missing key").red()
    };
    format!("{} | {}", upstream, api_key)
}

pub async fn doctor(
    config_path: &str,
    mode_id: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    let config_exists = Path::new(config_path).exists();
    let modes = load_mode_views(config_path).await?;
    let default_mode = effective_default_mode_id(&modes).map(ToOwned::to_owned);
    let (is_healthy, health) = gateway_health_status(bind_override).await;
    let bind_display = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);

    println!("{}", style("🩺 UniGateway Doctor").bold());
    println!("-------------------");
    println!("Config Path:   {}", config_path);
    println!(
        "Config Status: {}",
        if config_exists {
            style("✓ present").green()
        } else {
            style("✗ missing (using in-memory defaults if started)").red()
        }
    );
    println!("Gateway Bind:   {}", bind_display);
    println!(
        "Gateway Health: {}",
        if is_healthy {
            style(format!("✓ {}", health)).green()
        } else {
            style(format!("✗ {}", health)).red()
        }
    );

    if !is_healthy {
        println!(
            "\n💡 Tip: The gateway is not running. Start it with `ug serve` to enable LLM routing."
        );
    }

    if modes.is_empty() {
        println!("\nModes: {}", style("none").yellow());
        println!("Next step: run `ug guide` to set up your first provider.");
        return Ok(());
    }

    let selected: Vec<&ModeView> = if let Some(mode_id) = mode_id {
        vec![select_mode(&modes, Some(mode_id))?]
    } else {
        modes.iter().collect()
    };

    println!("\nModes Checked: {}", selected.len());
    for mode in selected {
        let protocols = supported_protocols(mode);
        let active_keys = mode.keys.iter().filter(|key| key.is_active).count();
        let is_default = default_mode.as_deref() == Some(mode.id.as_str());

        println!("\n- {} ({})", style(&mode.id).bold(), mode.name);
        if is_default {
            println!("  {}", style("★ Default Mode").yellow());
        }
        println!("  Routing:   {}", mode.routing_strategy);
        println!(
            "  Auth:      {} / {} active keys",
            active_keys,
            mode.keys.len()
        );
        let protocol_list = protocols.join(", ");
        println!(
            "  Protocols: {}",
            if protocols.is_empty() {
                "none"
            } else {
                &protocol_list
            }
        );

        if active_keys == 0 {
            println!(
                "  {}",
                style("⚠️ Warning: No active gateway key for this mode. Requests will fail.")
                    .yellow()
            );
        }

        for protocol in protocols {
            let providers = mode_providers_for(mode, protocol);
            println!(
                "  {} Route: {}",
                protocol.to_uppercase(),
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
                    "    → {}: {} [family={}]",
                    provider.name,
                    resolved_base_url,
                    family_id.as_deref().unwrap_or("-"),
                );
                println!("      Status: {}", provider_readiness(provider));
            }
        }

        let disabled = mode
            .providers
            .iter()
            .filter(|provider| !provider.is_enabled)
            .count();
        if disabled > 0 {
            println!("  ℹ️ Note: {} bound provider(s) are disabled", disabled);
        }

        println!("\n  Next Steps:");
        println!(
            "    ug route explain {}  # See detailed routing logic",
            mode.id
        );
        println!(
            "    ug test --mode {}     # Send a smoke test request",
            mode.id
        );
        println!(
            "    ug integrations --mode {} # Get tool config snippets",
            mode.id
        );
    }

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

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message(format!("Testing mode '{}' via {}...", mode.id, protocol));
    pb.enable_steady_tick(Duration::from_millis(100));

    let response = request.send().await;

    pb.finish_and_clear();

    let response = response.with_context(|| {
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
        "{} Mode '{}' passed {} smoke test: {}",
        style("✓").green(),
        mode.id,
        protocol,
        style(summarize_response_text(&body)).dim()
    );
    Ok(())
}
