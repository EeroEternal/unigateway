use anyhow::Result;
use std::fmt::Write as _;

use crate::config::{ModeProvider, ModeView};
use crate::routing::resolve_upstream;

use super::super::modes::{load_mode_views, mode_providers_for, select_mode, supported_protocols};

pub(crate) fn route_strategy_summary(mode: &ModeView, providers: &[&ModeProvider]) -> String {
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

pub(crate) fn render_route_explanation(mode: &ModeView) -> String {
    let mut out = String::new();
    let protocols = supported_protocols(mode);

    let _ = writeln!(&mut out, "mode:     {} ({})", mode.id, mode.name);
    let _ = writeln!(&mut out, "routing:  {}", mode.routing_strategy);
    let _ = writeln!(
        &mut out,
        "protocol: {}",
        if protocols.is_empty() {
            "none".to_string()
        } else {
            protocols.join(", ")
        }
    );

    let openai_providers = mode_providers_for(mode, "openai");
    let strategy = route_strategy_summary(mode, &openai_providers);
    let _ = writeln!(&mut out, "Effective strategy: {}", strategy);

    if protocols.is_empty() {
        let _ = writeln!(&mut out, "no enabled providers");
        return out.trim_end().to_string();
    }

    for protocol in protocols {
        let providers = mode_providers_for(mode, protocol);
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "{}:", protocol);

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

            let _ = writeln!(out, "  {}. {}", index + 1, provider.name);
            let _ = writeln!(out, "     type:   {}", provider.provider_type);
            if let Some(eid) = &provider.endpoint_id {
                let _ = writeln!(out, "     id:     {}", eid);
            }
            if let Some(model) = &provider.default_model {
                let _ = writeln!(out, "     model:  {}", model);
            }
            let _ = writeln!(&mut out, "     url:    {}", resolved_base_url);
            if let Some(family) = family_id {
                let _ = writeln!(out, "     family: {}", family);
            }
            let _ = writeln!(&mut out, "     prio:   {}", provider.priority);
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

pub async fn explain_route(config_path: &str, mode_id: Option<&str>) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id)?;
    println!("{}", render_route_explanation(mode));
    Ok(())
}
