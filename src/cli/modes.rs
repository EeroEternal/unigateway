use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::{config::GatewayState, types::AppConfig};

#[derive(Clone)]
pub(crate) struct ModeProvider {
    pub(crate) name: String,
    pub(crate) provider_type: String,
    pub(crate) endpoint_id: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) default_model: Option<String>,
    pub(crate) model_mapping: Option<String>,
    pub(crate) has_api_key: bool,
    pub(crate) is_enabled: bool,
    pub(crate) priority: i64,
}

#[derive(Clone)]
pub(crate) struct ModeKey {
    pub(crate) key: String,
    pub(crate) is_active: bool,
    pub(crate) quota_limit: Option<i64>,
    pub(crate) qps_limit: Option<f64>,
    pub(crate) concurrency_limit: Option<i64>,
}

#[derive(Clone)]
pub(crate) struct ModeView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) is_default: bool,
    pub(crate) routing_strategy: String,
    pub(crate) providers: Vec<ModeProvider>,
    pub(crate) keys: Vec<ModeKey>,
}

pub(crate) fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}…{}", &key[..4], &key[key.len() - 4..])
}

pub(crate) fn format_i64_limit(limit: Option<i64>) -> String {
    limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

pub(crate) fn format_f64_limit(limit: Option<f64>) -> String {
    limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unlimited".to_string())
}

pub(crate) fn user_bind_address(bind: &str) -> String {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return bind.to_string();
    };

    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        _ => host,
    };
    format!("{host}:{port}")
}

pub(crate) fn user_openai_base_url(bind_override: Option<&str>) -> String {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    format!("http://{}/v1", user_bind_address(&bind))
}

pub(crate) fn user_anthropic_base_url(bind_override: Option<&str>) -> String {
    let bind = bind_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| AppConfig::from_env().bind);
    format!("http://{}", user_bind_address(&bind))
}

pub(crate) async fn load_mode_views(config_path: &str) -> Result<Vec<ModeView>> {
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

pub(crate) fn supported_protocols(mode: &ModeView) -> Vec<&'static str> {
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

pub(crate) fn mode_providers_for<'a>(mode: &'a ModeView, protocol: &str) -> Vec<&'a ModeProvider> {
    mode.providers
        .iter()
        .filter(|provider| provider.is_enabled && provider.provider_type == protocol)
        .collect()
}

pub(crate) fn effective_default_mode_id(modes: &[ModeView]) -> Option<&str> {
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

pub(crate) fn select_mode<'a>(
    modes: &'a [ModeView],
    requested_mode: Option<&str>,
) -> Result<&'a ModeView> {
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

pub(crate) fn provider_default_model<'a>(provider: &'a ModeProvider, fallback: &'a str) -> &'a str {
    provider
        .default_model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or(fallback)
}

pub(crate) fn pick_mode_key(mode: &ModeView) -> Result<String> {
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

pub(crate) fn pick_mode_protocol<'a>(
    mode: &'a ModeView,
    requested: Option<&str>,
) -> Result<&'a str> {
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

pub async fn use_mode(config_path: &str, mode_id: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.set_default_mode(mode_id).await?;
    state.persist_if_dirty().await?;
    println!("Default mode set to '{}'", mode_id);
    Ok(())
}
